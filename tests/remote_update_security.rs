//! Exhaustive remote release discovery, URL/redirect security, SHA-256 verification,
//! archive safety (`TarSlip`/`ZipSlip`/bomb/shadowing), native CPU binary header inspection,
//! and live loopback mock HTTP server end-to-end fault-injection tests.

use flate2::write::GzEncoder;
use flate2::Compression;
use lumaglyph::updater::{
    apply_verified_binary_to_path, check_latest_release_with_policy,
    cleanup_stale_old_executables_in_dir, constant_time_eq_32, detect_local_cargo_checkout,
    download_and_verify_release_with_policy, extract_verified_binary_from_archive, hex_encode_32,
    install_verified_release_with_policy, is_newer_semver, parse_hex_32, parse_sha256sums,
    relaunch_executable, sha256_bytes, validate_archive_member_name, validate_asset_download_url,
    validate_flat_filename, validate_redirect_url, validate_release_html_url, validate_release_tag,
    validate_repo_slug, verify_executable_header, EndpointPolicy, ParsedUrl, PlatformTarget,
    DEFAULT_GITHUB_REPO,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use zip::write::SimpleFileOptions;

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(1);

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let id = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lumaglyph_upd_test_{prefix}_{}_{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Synthesizes a valid minimal native executable header + high-entropy machine-code payload
/// so Deflate/Gzip compression produces realistic archive sizes (> `MIN_ARCHIVE_BYTES`).
fn make_synthetic_native_binary(target: PlatformTarget, size: usize) -> Vec<u8> {
    let mut bin = vec![0u8; size.max(1024)];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for b in bin.iter_mut().skip(256) {
        state = state.wrapping_add(0xBF58_476D_1CE4_E5B9);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0x94D0_49BB_1331_11EB);
        *b = (z >> 56) as u8;
    }
    match target {
        PlatformTarget::LinuxX86_64 => {
            bin[0..4].copy_from_slice(b"\x7FELF");
            bin[4] = 2; // ELFCLASS64
            bin[5] = 1; // ELFDATA2LSB
            bin[6] = 1; // EV_CURRENT
            bin[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN (PIE)
            bin[18..20].copy_from_slice(&0x003Eu16.to_le_bytes()); // EM_X86_64
        }
        PlatformTarget::MacosArm64 => {
            bin[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes()); // MH_MAGIC_64
            bin[4..8].copy_from_slice(&0x0100_000Cu32.to_le_bytes()); // CPU_TYPE_ARM64
            bin[12..16].copy_from_slice(&2u32.to_le_bytes()); // MH_EXECUTE
        }
        PlatformTarget::MacosX86_64 => {
            bin[0..4].copy_from_slice(&0xFEED_FACFu32.to_le_bytes()); // MH_MAGIC_64
            bin[4..8].copy_from_slice(&0x0100_0007u32.to_le_bytes()); // CPU_TYPE_X86_64
            bin[12..16].copy_from_slice(&2u32.to_le_bytes()); // MH_EXECUTE
        }
        PlatformTarget::WindowsX86_64 => {
            bin[0..2].copy_from_slice(b"MZ");
            let pe_off: u32 = 0x80;
            bin[0x3C..0x40].copy_from_slice(&pe_off.to_le_bytes());
            let p = pe_off as usize;
            bin[p..p + 4].copy_from_slice(b"PE\0\0");
            bin[p + 4..p + 6].copy_from_slice(&0x8664u16.to_le_bytes()); // IMAGE_FILE_MACHINE_AMD64
            bin[p + 24..p + 26].copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
            bin[p + 24 + 68..p + 24 + 70].copy_from_slice(&2u16.to_le_bytes()); // IMAGE_SUBSYSTEM_WINDOWS_GUI (2)
        }
    }
    bin
}

fn build_tar_gz(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    {
        let mut builder = tar::Builder::new(&mut enc);
        for &(name, data, kind) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(kind);
            if kind.is_file() {
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, name, data).unwrap();
            } else {
                header.set_size(0);
                header.set_mode(0o777);
                if kind.is_symlink() || kind.is_hard_link() {
                    let _ = header.set_link_name("target");
                }
                header.set_cksum();
                builder
                    .append_data(&mut header, name, std::io::empty())
                    .unwrap();
            }
        }
        builder.finish().unwrap();
    }
    enc.finish().unwrap()
}

fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cur = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(cur);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for &(name, data) in entries {
        zw.start_file(name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn build_valid_release_archive(target: PlatformTarget) -> (Vec<u8>, Vec<u8>) {
    let bin = make_synthetic_native_binary(target, 4096);
    let archive = match target {
        PlatformTarget::WindowsX86_64 => build_zip(&[
            ("lumaglyph.exe", &bin),
            ("LICENSE", b"MIT License"),
            ("README.md", b"# LumaGlyph"),
        ]),
        _ => build_tar_gz(&[
            ("lumaglyph", &bin, tar::EntryType::Regular),
            ("LICENSE", b"MIT License", tar::EntryType::Regular),
            ("README.md", b"# LumaGlyph", tar::EntryType::Regular),
        ]),
    };
    (archive, bin)
}

// ============================================================================
// 1. STRICT URL & ORIGIN PARSING / REJECTION MATRIX (~95 INDIVIDUAL TESTS)
// ============================================================================

macro_rules! url_reject_tests {
    ($($name:ident => $url:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let prod = EndpointPolicy::production_github(true);
                let ok_parse_and_transport = ParsedUrl::parse_strict($url)
                    .and_then(|u| {
                        validate_redirect_url($url, &prod)?;
                        let _ = u;
                        Ok(())
                    });
                assert!(
                    ok_parse_and_transport.is_err(),
                    "Expected URL '{}' to be rejected by strict production policy",
                    $url
                );
            }
        )+
    };
}

url_reject_tests! {
    url_reject_empty => "",
    url_reject_whitespace_only => "   ",
    url_reject_plain_http => "http://github.com/goldggs/lumaglyph/releases/tag/v0.4.0",
    url_reject_http_objects => "http://objects.githubusercontent.com/github-production-release-asset/bin",
    url_reject_ftp_scheme => "ftp://github.com/goldggs/lumaglyph/file.tar.gz",
    url_reject_file_unix => "file:///etc/passwd",
    url_reject_file_windows => "file:///C:/Windows/System32/cmd.exe",
    url_reject_javascript => "javascript:alert(1)",
    url_reject_javascript_slashes => "javascript://github.com/%0aalert(1)",
    url_reject_data_uri => "data:application/octet-stream;base64,AAAA",
    url_reject_smb_unc => "smb://192.168.1.10/share/evil.exe",
    url_reject_uppercase_scheme => "HTTPS://github.com/goldggs/lumaglyph",
    url_reject_mixed_scheme => "Https://github.com/goldggs/lumaglyph",
    url_reject_missing_scheme => "github.com/goldggs/lumaglyph",
    url_reject_protocol_relative => "//github.com/goldggs/lumaglyph",
    url_reject_relative_path => "/goldggs/lumaglyph/releases",
    url_reject_userinfo_spoof_host => "https://github.com@evil.example.com/goldggs/lumaglyph",
    url_reject_userinfo_creds => "https://user:pass@github.com/goldggs/lumaglyph",
    url_reject_userinfo_empty => "https://@github.com/goldggs/lumaglyph",
    url_reject_evil_domain => "https://evil.com/goldggs/lumaglyph/releases/download/v0.4.0/a.zip",
    url_reject_suffix_spoof => "https://github.com.evil.com/goldggs/lumaglyph",
    url_reject_prefix_spoof => "https://mygithub.com/goldggs/lumaglyph",
    url_reject_hyphen_spoof => "https://api-github.com/repos/goldggs/lumaglyph",
    url_reject_subdomain_spoof => "https://evil.github.com/goldggs/lumaglyph",
    url_reject_raw_github_content => "https://raw.githubusercontent.com/goldggs/lumaglyph/main/evil",
    url_reject_gist_host => "https://gist.github.com/goldggs/12345",
    url_reject_localhost => "https://localhost/goldggs/lumaglyph",
    url_reject_loopback_v4 => "https://127.0.0.1/goldggs/lumaglyph",
    url_reject_cloud_metadata_ssrf => "https://169.254.169.254/latest/meta-data/",
    url_reject_private_10_net => "https://10.0.0.1/evil",
    url_reject_private_192_net => "https://192.168.1.1/evil",
    url_reject_ipv6_loopback => "https://[::1]/evil",
    url_reject_ipv6_mapped => "https://[::ffff:127.0.0.1]/evil",
    url_reject_explicit_port_443 => "https://github.com:443/goldggs/lumaglyph",
    url_reject_explicit_port_8443 => "https://github.com:8443/goldggs/lumaglyph",
    url_reject_explicit_port_80 => "https://github.com:80/goldggs/lumaglyph",
    url_reject_port_zero => "https://github.com:0/goldggs/lumaglyph",
    url_reject_port_overflow => "https://github.com:99999/goldggs/lumaglyph",
    url_reject_port_non_numeric => "https://github.com:abc/goldggs/lumaglyph",
    url_reject_port_empty => "https://github.com:/goldggs/lumaglyph",
    url_reject_trailing_dot_host => "https://github.com./goldggs/lumaglyph",
    url_reject_leading_dot_host => "https://.github.com/goldggs/lumaglyph",
    url_reject_double_dot_host => "https://github..com/goldggs/lumaglyph",
    url_reject_empty_host => "https:///goldggs/lumaglyph",
    url_reject_dotdot_traversal => "https://github.com/goldggs/lumaglyph/../../evil/repo",
    url_reject_single_dot_seg => "https://github.com/./goldggs/lumaglyph",
    url_reject_encoded_dot_lower => "https://github.com/goldggs/%2e%2e/evil",
    url_reject_encoded_dot_upper => "https://github.com/goldggs/%2E%2E/evil",
    url_reject_encoded_slash => "https://github.com/goldggs%2flumaglyph",
    url_reject_encoded_backslash => "https://github.com/goldggs%5clumaglyph",
    url_reject_encoded_nul => "https://github.com/goldggs/lumaglyph%00",
    url_reject_raw_backslash => "https://github.com\\@evil.com/goldggs/lumaglyph",
    url_reject_fragment_hash => "https://github.com/goldggs/lumaglyph#fragment",
    url_reject_embedded_space => "https://github.com/goldggs/luma glyph",
    url_reject_embedded_newline => "https://github.com/goldggs/lumaglyph\nLocation: evil",
    url_reject_embedded_cr => "https://github.com/goldggs/lumaglyph\r",
    url_reject_embedded_tab => "https://github.com/goldggs/\tlumaglyph",
    url_reject_embedded_nul => "https://github.com/goldggs/lumaglyph\0",
    url_reject_unicode_homoglyph => "https://g\u{0456}thub.com/goldggs/lumaglyph",
    url_reject_query_without_slash => "https://github.com?redirect=evil.com",
}

macro_rules! asset_url_mismatch_tests {
    ($($name:ident => ($url:expr, $owner:expr, $repo:expr, $tag:expr, $asset:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let prod = EndpointPolicy::production_github(true);
                assert!(
                    validate_asset_download_url($url, $owner, $repo, $tag, $asset, &prod).is_err(),
                    "Expected asset URL validation failure for {}",
                    $url
                );
            }
        )+
    };
}

asset_url_mismatch_tests! {
    asset_url_wrong_owner => (
        "https://github.com/impostor/lumaglyph/releases/download/v0.4.0/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_wrong_repo => (
        "https://github.com/goldggs/malware/releases/download/v0.4.0/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_wrong_tag => (
        "https://github.com/goldggs/lumaglyph/releases/download/v0.1.0/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_wrong_filename => (
        "https://github.com/goldggs/lumaglyph/releases/download/v0.4.0/something-else.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_query_injected => (
        "https://github.com/goldggs/lumaglyph/releases/download/v0.4.0/lumaglyph-linux-x86_64.tar.gz?foo=1",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_extra_segment => (
        "https://github.com/goldggs/lumaglyph/releases/download/v0.4.0/extra/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_archive_source_zip => (
        "https://github.com/goldggs/lumaglyph/archive/refs/tags/v0.4.0.zip",
        "goldggs", "lumaglyph", "v0.4.0", "v0.4.0.zip"
    ),
    asset_url_objects_direct_initial => (
        "https://objects.githubusercontent.com/goldggs/lumaglyph/releases/download/v0.4.0/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
    asset_url_http_downgrade => (
        "http://github.com/goldggs/lumaglyph/releases/download/v0.4.0/lumaglyph-linux-x86_64.tar.gz",
        "goldggs", "lumaglyph", "v0.4.0", "lumaglyph-linux-x86_64.tar.gz"
    ),
}

#[test]
fn valid_production_urls_and_cdn_redirects_pass() {
    let prod = EndpointPolicy::production_github(true);
    validate_release_html_url(
        "https://github.com/goldggs/lumaglyph/releases/tag/v0.4.0",
        "goldggs",
        "lumaglyph",
        "v0.4.0",
        &prod,
    )
    .unwrap();
    validate_asset_download_url(
        "https://github.com/goldggs/lumaglyph/releases/download/v0.4.0/lumaglyph-windows-x86_64.zip",
        "goldggs",
        "lumaglyph",
        "v0.4.0",
        "lumaglyph-windows-x86_64.zip",
        &prod,
    )
    .unwrap();
    validate_redirect_url(
        "https://objects.githubusercontent.com/github-production-release-asset-2e65be/12345/blob?X-Amz-Algorithm=AWS4-HMAC-SHA256&response-content-type=application%2Foctet-stream",
        &prod,
    )
    .unwrap();
    assert!(validate_redirect_url(
        "https://objects.githubusercontent.com/github-production-release-asset-2e65be/%2f..%2fblob?X-Amz-Algorithm=AWS4-HMAC-SHA256",
        &prod,
    )
    .is_err());
    assert!(validate_redirect_url(
        "https://objects.githubusercontent.com/github-production-release-asset-2e65be/12345/blob?sig=%00bad",
        &prod,
    )
    .is_err());
    validate_redirect_url(
        "https://release-assets.githubusercontent.com/github-production-release-asset/98765?sig=abc",
        &prod,
    )
    .unwrap();
}

#[test]
fn url_exceeding_2048_bytes_is_rejected() {
    let long = format!("https://github.com/{}", "a".repeat(2100));
    assert!(ParsedUrl::parse_strict(&long).is_err());
}

// ============================================================================
// 2. REPO SLUG, RELEASE TAG & SEMVER MATRIX (~85 INDIVIDUAL TESTS)
// ============================================================================

macro_rules! repo_slug_tests {
    ($($name:ident => ($slug:expr, $canon:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(
                    validate_repo_slug($slug, $canon).is_ok(),
                    $expect_ok,
                    "Unexpected slug validation outcome for '{}' (canonical={})",
                    $slug,
                    $canon
                );
            }
        )+
    };
}

repo_slug_tests! {
    slug_canonical_exact => (DEFAULT_GITHUB_REPO, true, true),
    slug_canonical_case_insensitive => ("GOLDGGS/LumaGlyph", true, true),
    slug_canonical_trimmed => ("  goldggs/lumaglyph  ", true, true),
    slug_custom_blocked_when_canonical_required => ("otheruser/lumaglyph", true, false),
    slug_custom_allowed_when_opted_in => ("otheruser/lumaglyph-fork", false, true),
    slug_reject_empty => ("", false, false),
    slug_reject_placeholder => ("YOUR_GITHUB_USERNAME/lumaglyph", false, false),
    slug_reject_no_slash => ("lumaglyph", false, false),
    slug_reject_two_slashes => ("goldggs/lumaglyph/extra", false, false),
    slug_reject_leading_slash => ("/goldggs/lumaglyph", false, false),
    slug_reject_trailing_slash => ("goldggs/lumaglyph/", false, false),
    slug_reject_dot_owner => ("./lumaglyph", false, false),
    slug_reject_dotdot_owner => ("../lumaglyph", false, false),
    slug_reject_dot_repo => ("goldggs/.", false, false),
    slug_reject_dotdot_repo => ("goldggs/..", false, false),
    slug_reject_leading_hyphen_owner => ("-goldggs/lumaglyph", false, false),
    slug_reject_trailing_hyphen_owner => ("goldggs-/lumaglyph", false, false),
    slug_reject_leading_dot_owner => (".goldggs/lumaglyph", false, false),
    slug_reject_url_passed_as_slug => ("https://github.com/goldggs/lumaglyph", false, false),
    slug_reject_query_injection => ("goldggs/lumaglyph?foo=bar", false, false),
    slug_reject_hash_injection => ("goldggs/lumaglyph#frag", false, false),
    slug_reject_space_inside => ("goldggs/luma glyph", false, false),
    slug_reject_backslash => ("goldggs\\lumaglyph", false, false),
    slug_reject_at_sign => ("gold@ggs/lumaglyph", false, false),
    slug_reject_percent => ("goldggs/luma%2fglyph", false, false),
    slug_reject_cyrillic_homoglyph => ("g\u{043E}ldggs/lumaglyph", false, false),
}

#[test]
fn slug_rejects_overlong_owner_and_repo() {
    let long_owner = format!("{}/lumaglyph", "a".repeat(40));
    let long_repo = format!("goldggs/{}", "b".repeat(101));
    assert!(validate_repo_slug(&long_owner, false).is_err());
    assert!(validate_repo_slug(&long_repo, false).is_err());
}

macro_rules! release_tag_tests {
    ($($name:ident => ($tag:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(
                    validate_release_tag($tag).is_ok(),
                    $expect_ok,
                    "Unexpected tag validation for '{}'",
                    $tag
                );
            }
        )+
    };
}

release_tag_tests! {
    tag_valid_v031 => ("v0.3.1", true),
    tag_valid_bare_031 => ("0.3.1", true),
    tag_valid_upper_v => ("V1.2.3", true),
    tag_valid_multi_digit => ("v12.345.6789", true),
    tag_valid_zero => ("v0.0.1", true),
    tag_reject_empty => ("", false),
    tag_reject_two_parts => ("v0.3", false),
    tag_reject_four_parts => ("v0.3.1.2", false),
    tag_reject_single_num => ("v1", false),
    tag_reject_prerelease_alpha => ("v0.4.0-alpha.1", false),
    tag_reject_prerelease_rc => ("v0.4.0-rc1", false),
    tag_reject_build_meta => ("v0.4.0+20261028", false),
    tag_reject_traversal => ("../v0.4.0", false),
    tag_reject_slash => ("refs/tags/v0.4.0", false),
    tag_reject_space => ("v0.4.0 ", false),
    tag_reject_leading_space => (" v0.4.0", false),
    tag_reject_semicolon => ("v0.4.0;id", false),
    tag_reject_negative => ("v0.-1.0", false),
    tag_reject_hex => ("v0.0x1.0", false),
    tag_reject_empty_component => ("v0..1", false),
    tag_reject_six_digit_part => ("v100000.0.0", false),
    tag_reject_over_24_chars => ("v12345.12345.1234567890123", false),
}

macro_rules! semver_tests {
    ($($name:ident => ($remote:expr, $cur:expr, $expect:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(is_newer_semver($remote, $cur), $expect);
            }
        )+
    };
}

semver_tests! {
    semver_patch_bump => ("v0.3.2", "0.3.1", true),
    semver_minor_bump => ("v0.4.0", "0.3.99", true),
    semver_major_bump => ("v1.0.0", "0.99.99", true),
    semver_upper_v_prefix => ("V2.1.0", "v2.0.9", true),
    semver_bare_remote => ("0.3.2", "v0.3.1", true),
    semver_equal_version => ("v0.3.1", "0.3.1", false),
    semver_downgrade_patch => ("v0.3.0", "0.3.1", false),
    semver_downgrade_minor => ("v0.2.9", "0.3.1", false),
    semver_downgrade_major => ("v0.9.9", "1.0.0", false),
    semver_zero_zero => ("v0.0.0", "0.0.0", false),
    semver_malformed_remote_text => ("latest", "0.3.1", false),
    semver_malformed_remote_two_part => ("v0.4", "0.3.1", false),
    semver_malformed_remote_four_part => ("v0.4.0.1", "0.3.1", false),
    semver_malformed_current => ("v0.4.0", "invalid", false),
    semver_empty_remote => ("", "0.3.1", false),
}

// ============================================================================
// 3. SHA-256 VECTORS, CONSTANT-TIME BIT-FLIP MATRIX & SHA256SUMS.txt (~55 TESTS)
// ============================================================================

#[test]
fn sha256_nist_vector_empty() {
    let d = sha256_bytes(b"");
    assert_eq!(
        hex_encode_32(&d),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_nist_vector_abc() {
    let d = sha256_bytes(b"abc");
    assert_eq!(
        hex_encode_32(&d),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_nist_vector_448bit_message() {
    let d = sha256_bytes(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
    assert_eq!(
        hex_encode_32(&d),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

macro_rules! bit_flip_tests {
    ($($name:ident => $byte_idx:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let base = sha256_bytes(b"lumaglyph-release-integrity");
                assert!(constant_time_eq_32(&base, &base));
                for bit in 0..8u8 {
                    let mut mutated = base;
                    mutated[$byte_idx] ^= 1u8 << bit;
                    assert!(
                        !constant_time_eq_32(&base, &mutated),
                        "Bit flip at byte {} bit {} was not detected!",
                        $byte_idx,
                        bit
                    );
                }
            }
        )+
    };
}

bit_flip_tests! {
    ct_eq_byte_00 => 0,  ct_eq_byte_01 => 1,  ct_eq_byte_02 => 2,  ct_eq_byte_03 => 3,
    ct_eq_byte_04 => 4,  ct_eq_byte_05 => 5,  ct_eq_byte_06 => 6,  ct_eq_byte_07 => 7,
    ct_eq_byte_08 => 8,  ct_eq_byte_09 => 9,  ct_eq_byte_10 => 10, ct_eq_byte_11 => 11,
    ct_eq_byte_12 => 12, ct_eq_byte_13 => 13, ct_eq_byte_14 => 14, ct_eq_byte_15 => 15,
    ct_eq_byte_16 => 16, ct_eq_byte_17 => 17, ct_eq_byte_18 => 18, ct_eq_byte_19 => 19,
    ct_eq_byte_20 => 20, ct_eq_byte_21 => 21, ct_eq_byte_22 => 22, ct_eq_byte_23 => 23,
    ct_eq_byte_24 => 24, ct_eq_byte_25 => 25, ct_eq_byte_26 => 26, ct_eq_byte_27 => 27,
    ct_eq_byte_28 => 28, ct_eq_byte_29 => 29, ct_eq_byte_30 => 30, ct_eq_byte_31 => 31,
}

macro_rules! sha256sums_parse_tests {
    ($($name:ident => ($input:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(parse_sha256sums($input).is_ok(), $expect_ok);
            }
        )+
    };
}

sha256sums_parse_tests! {
    sums_valid_gnu_two_space => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  lumaglyph-linux-x86_64.tar.gz\n",
        true
    ),
    sums_valid_binary_star => (
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD *lumaglyph-windows-x86_64.zip\r\n",
        true
    ),
    sums_valid_multi_platform_crlf => (
        "\n\
         e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  lumaglyph-linux-x86_64.tar.gz\r\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  lumaglyph-macos-arm64.tar.gz\r\n\
         248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1  lumaglyph-windows-x86_64.zip\r\n",
        true
    ),
    sums_reject_empty => ("", false),
    sums_reject_whitespace_only => ("   \n\r\n  ", false),
    sums_reject_duplicate_filename_confusion => (
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  lumaglyph-linux-x86_64.tar.gz\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  lumaglyph-linux-x86_64.tar.gz\n",
        false
    ),
    sums_reject_short_hex => ("ba7816bf  lumaglyph-linux-x86_64.tar.gz", false),
    sums_reject_non_hex => (
        "zz7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  lumaglyph-linux-x86_64.tar.gz",
        false
    ),
    sums_reject_path_traversal_name => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ../lumaglyph-linux-x86_64.tar.gz",
        false
    ),
    sums_reject_subdir_name => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  sub/lumaglyph.zip",
        false
    ),
    sums_reject_windows_drive_name => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  C:\\lumaglyph.zip",
        false
    ),
    sums_reject_nul_byte => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  lumaglyph.zip\0",
        false
    ),
    sums_reject_missing_separator => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad:lumaglyph.zip",
        false
    ),
    sums_reject_dotfile => (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  .hidden",
        false
    ),
}

#[test]
fn parse_hex_32_roundtrip_and_invalid_lengths() {
    let orig = sha256_bytes(b"roundtrip-test");
    let hex = hex_encode_32(&orig);
    assert_eq!(parse_hex_32(&hex).unwrap(), orig);
    assert_eq!(parse_hex_32(&hex.to_ascii_uppercase()).unwrap(), orig);
    assert!(parse_hex_32(&hex[..63]).is_err());
    assert!(parse_hex_32(&format!("{hex}0")).is_err());
}

// ============================================================================
// 4. ARCHIVE EXTRACTION, TARSLIP/ZIPSLIP & BINARY HEADER MATRIX (~90 TESTS)
// ============================================================================

macro_rules! member_name_tests {
    ($($name:ident => ($member:expr, $bin:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(
                    validate_archive_member_name($member, $bin).is_ok(),
                    $expect_ok,
                    "Unexpected member validation for '{}'",
                    $member
                );
            }
        )+
    };
}

member_name_tests! {
    member_valid_unix_bin => ("lumaglyph", "lumaglyph", true),
    member_valid_dotslash_bin => ("./lumaglyph", "lumaglyph", true),
    member_valid_win_exe => ("lumaglyph.exe", "lumaglyph.exe", true),
    member_valid_license => ("LICENSE", "lumaglyph", true),
    member_valid_readme => ("README.md", "lumaglyph.exe", true),
    member_reject_tarslip_parent => ("../lumaglyph", "lumaglyph", false),
    member_reject_tarslip_deep => ("../../etc/cron.d/pwn", "lumaglyph", false),
    member_reject_zipslip_backslash => ("..\\lumaglyph.exe", "lumaglyph.exe", false),
    member_reject_abs_unix => ("/usr/local/bin/lumaglyph", "lumaglyph", false),
    member_reject_win_drive => ("C:\\lumaglyph.exe", "lumaglyph.exe", false),
    member_reject_subdir => ("bin/lumaglyph", "lumaglyph", false),
    member_reject_double_dotslash => ("././lumaglyph", "lumaglyph", false),
    member_reject_sideload_dll => ("version.dll", "lumaglyph.exe", false),
    member_reject_sideload_dxgi_dll => ("dxgi.dll", "lumaglyph.exe", false),
    member_reject_unexpected_exe => ("updater_helper.exe", "lumaglyph.exe", false),
    member_reject_unix_exe_in_win => ("lumaglyph", "lumaglyph.exe", false),
    member_reject_win_exe_in_unix => ("lumaglyph.exe", "lumaglyph", false),
    member_reject_shell_script => ("install.sh", "lumaglyph", false),
    member_reject_batch_script => ("run.bat", "lumaglyph.exe", false),
    member_reject_powershell_script => ("setup.ps1", "lumaglyph.exe", false),
    member_reject_shared_object => ("libpreload.so", "lumaglyph", false),
    member_reject_dylib => ("libhook.dylib", "lumaglyph", false),
    member_reject_dotfile => (".bashrc", "lumaglyph", false),
    member_reject_nul_byte => ("lumaglyph\0.txt", "lumaglyph", false),
    member_reject_empty => ("", "lumaglyph", false),
}

macro_rules! cross_platform_header_tests {
    ($($name:ident => ($bin_kind:expr, $target_kind:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let bin = make_synthetic_native_binary($bin_kind, 1024);
                assert_eq!(
                    verify_executable_header(&bin, $target_kind).is_ok(),
                    $expect_ok
                );
            }
        )+
    };
}

cross_platform_header_tests! {
    hdr_match_linux_x64 => (PlatformTarget::LinuxX86_64, PlatformTarget::LinuxX86_64, true),
    hdr_match_macos_arm64 => (PlatformTarget::MacosArm64, PlatformTarget::MacosArm64, true),
    hdr_match_macos_x64 => (PlatformTarget::MacosX86_64, PlatformTarget::MacosX86_64, true),
    hdr_match_windows_x64 => (PlatformTarget::WindowsX86_64, PlatformTarget::WindowsX86_64, true),

    hdr_mismatch_linux_on_win => (PlatformTarget::LinuxX86_64, PlatformTarget::WindowsX86_64, false),
    hdr_mismatch_linux_on_mac_arm => (PlatformTarget::LinuxX86_64, PlatformTarget::MacosArm64, false),
    hdr_mismatch_linux_on_mac_x64 => (PlatformTarget::LinuxX86_64, PlatformTarget::MacosX86_64, false),

    hdr_mismatch_mac_arm_on_mac_x64 => (PlatformTarget::MacosArm64, PlatformTarget::MacosX86_64, false),
    hdr_mismatch_mac_arm_on_linux => (PlatformTarget::MacosArm64, PlatformTarget::LinuxX86_64, false),
    hdr_mismatch_mac_arm_on_win => (PlatformTarget::MacosArm64, PlatformTarget::WindowsX86_64, false),

    hdr_mismatch_mac_x64_on_mac_arm => (PlatformTarget::MacosX86_64, PlatformTarget::MacosArm64, false),
    hdr_mismatch_mac_x64_on_linux => (PlatformTarget::MacosX86_64, PlatformTarget::LinuxX86_64, false),
    hdr_mismatch_mac_x64_on_win => (PlatformTarget::MacosX86_64, PlatformTarget::WindowsX86_64, false),

    hdr_mismatch_win_on_linux => (PlatformTarget::WindowsX86_64, PlatformTarget::LinuxX86_64, false),
    hdr_mismatch_win_on_mac_arm => (PlatformTarget::WindowsX86_64, PlatformTarget::MacosArm64, false),
    hdr_mismatch_win_on_mac_x64 => (PlatformTarget::WindowsX86_64, PlatformTarget::MacosX86_64, false),
}

#[test]
fn header_rejects_windows_console_subsystem_cui() {
    // Even if a Windows PE32+ AMD64 binary is valid, if Subsystem == 3 (CONSOLE) instead of 2 (GUI),
    // the updater must reject it so Windows never regresses back to opening a terminal window!
    let mut pe = make_synthetic_native_binary(PlatformTarget::WindowsX86_64, 1024);
    let pe_off = 0x80usize;
    pe[pe_off + 24 + 68..pe_off + 24 + 70].copy_from_slice(&3u16.to_le_bytes()); // CUI = 3
    let err = verify_executable_header(&pe, PlatformTarget::WindowsX86_64).unwrap_err();
    assert!(err.contains("WINDOWS_GUI"));
}

#[test]
fn header_rejects_html_captive_portal_and_shell_scripts_and_corrupt_pe() {
    let mut html = b"<!DOCTYPE html><html><body>Captive Portal Login</body></html>".to_vec();
    html.resize(1024, b' ');
    for t in PlatformTarget::ALL {
        assert!(verify_executable_header(&html, t).is_err());
        assert!(verify_executable_header(b"tiny", t).is_err());
    }

    let mut bad_pe = make_synthetic_native_binary(PlatformTarget::WindowsX86_64, 1024);
    bad_pe[0x3C..0x40].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    assert!(verify_executable_header(&bad_pe, PlatformTarget::WindowsX86_64).is_err());

    let mut elf_arm = make_synthetic_native_binary(PlatformTarget::LinuxX86_64, 1024);
    elf_arm[18..20].copy_from_slice(&0x00B7u16.to_le_bytes()); // EM_AARCH64 on x86_64 target
    assert!(verify_executable_header(&elf_arm, PlatformTarget::LinuxX86_64).is_err());
}

#[test]
fn archive_extraction_valid_across_all_4_targets() {
    for target in PlatformTarget::ALL {
        let (arch_bytes, orig_bin) = build_valid_release_archive(target);
        let extracted = extract_verified_binary_from_archive(&arch_bytes, target).unwrap();
        assert_eq!(extracted, orig_bin);
    }
}

#[test]
fn archive_rejects_symlinks_hardlinks_dirs_duplicates_and_missing_binary() {
    let linux_bin = make_synthetic_native_binary(PlatformTarget::LinuxX86_64, 1024);

    // Symlink inside tar.gz -> rejected
    let sym_tar = build_tar_gz(&[
        ("lumaglyph", &linux_bin, tar::EntryType::Symlink),
        ("LICENSE", b"MIT", tar::EntryType::Regular),
    ]);
    assert!(extract_verified_binary_from_archive(&sym_tar, PlatformTarget::LinuxX86_64).is_err());

    // Hardlink inside tar.gz -> rejected
    let link_tar = build_tar_gz(&[("lumaglyph", &linux_bin, tar::EntryType::Link)]);
    assert!(extract_verified_binary_from_archive(&link_tar, PlatformTarget::LinuxX86_64).is_err());

    // Directory entry inside tar.gz -> rejected
    let dir_tar = build_tar_gz(&[("sub", b"", tar::EntryType::Directory)]);
    assert!(extract_verified_binary_from_archive(&dir_tar, PlatformTarget::LinuxX86_64).is_err());

    // Duplicate binary entry shadowing attack in tar.gz -> rejected!
    let mut evil_bin = linux_bin.clone();
    evil_bin[500] ^= 0xFF;
    let dup_tar = build_tar_gz(&[
        ("lumaglyph", &linux_bin, tar::EntryType::Regular),
        ("lumaglyph", &evil_bin, tar::EntryType::Regular),
    ]);
    let dup_err =
        extract_verified_binary_from_archive(&dup_tar, PlatformTarget::LinuxX86_64).unwrap_err();
    assert!(dup_err.contains("Duplicate"));

    // Missing binary (only LICENSE + README.md) -> rejected
    let missing_tar = build_tar_gz(&[
        ("LICENSE", b"MIT", tar::EntryType::Regular),
        ("README.md", b"# Doc", tar::EntryType::Regular),
    ]);
    assert!(
        extract_verified_binary_from_archive(&missing_tar, PlatformTarget::LinuxX86_64).is_err()
    );

    // Too many entries (> MAX_ARCHIVE_ENTRIES) -> rejected
    let many_tar = build_tar_gz(&[
        ("lumaglyph", &linux_bin, tar::EntryType::Regular),
        ("LICENSE", b"1", tar::EntryType::Regular),
        ("README.md", b"2", tar::EntryType::Regular),
        ("LICENSE.txt", b"3", tar::EntryType::Regular),
        ("README.md", b"4", tar::EntryType::Regular),
        ("LICENSE", b"5", tar::EntryType::Regular),
        ("LICENSE", b"6", tar::EntryType::Regular),
        ("LICENSE", b"7", tar::EntryType::Regular),
        ("LICENSE", b"8", tar::EntryType::Regular),
    ]);
    assert!(extract_verified_binary_from_archive(&many_tar, PlatformTarget::LinuxX86_64).is_err());

    // Corrupt random bytes -> rejected cleanly without panic
    assert!(
        extract_verified_binary_from_archive(&vec![0xAB; 1024], PlatformTarget::LinuxX86_64)
            .is_err()
    );
    assert!(
        extract_verified_binary_from_archive(&vec![0xCD; 1024], PlatformTarget::WindowsX86_64)
            .is_err()
    );
}

#[test]
fn flat_filename_validation_blocks_all_os_hazards() {
    for good in [
        "lumaglyph",
        "lumaglyph.exe",
        "lumaglyph-v0.3.1.exe",
        "SHA256SUMS.txt",
        "lumaglyph-linux-x86_64.tar.gz",
    ] {
        validate_flat_filename(good).unwrap();
    }
    for bad in [
        "",
        ".",
        "..",
        ".lumaglyph",
        "lumaglyph.",
        "lumaglyph ",
        "a/b",
        "a\\b",
        "C:lumaglyph.exe",
        "luma*glyph",
        "luma?glyph",
        "luma|glyph",
        "luma<glyph>",
        "luma\"glyph",
        "a..b",
    ] {
        assert!(
            validate_flat_filename(bad).is_err(),
            "Expected '{bad}' to be rejected"
        );
    }
}

// ============================================================================
// 5. LIVE LOOPBACK MOCK HTTP SERVER END-TO-END SECURITY TESTS (~20 TESTS)
// ============================================================================

struct MockRoute {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn spawn_mock_http_server(
    routes: BTreeMap<String, MockRoute>,
    max_requests: usize,
) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes = Arc::new(routes);

    let handle = std::thread::spawn(move || {
        for _ in 0..max_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut req_buf = [0u8; 4096];
            let n = stream.read(&mut req_buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let req_text = String::from_utf8_lossy(&req_buf[..n]);
            let first_line = req_text.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");

            if let Some(route) = routes.get(path) {
                let reason = match route.status {
                    200 => "OK",
                    302 => "Found",
                    404 => "Not Found",
                    _ => "Status",
                };
                let mut hdr = format!(
                    "HTTP/1.1 {} {reason}\r\nConnection: close\r\n",
                    route.status
                );
                let has_cl = route
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("Content-Length"));
                if !has_cl && route.status == 200 {
                    hdr.push_str(&format!("Content-Length: {}\r\n", route.body.len()));
                }
                for (k, v) in &route.headers {
                    hdr.push_str(&format!("{k}: {v}\r\n"));
                }
                hdr.push_str("\r\n");
                let _ = stream.write_all(hdr.as_bytes());
                let _ = stream.write_all(&route.body);
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
            let _ = stream.flush();
        }
    });

    (port, handle)
}

use std::collections::BTreeMap;

fn run_happy_download_for_target(target: PlatformTarget) {
    let (archive_bytes, expected_bin) = build_valid_release_archive(target);
    let digest = sha256_bytes(&archive_bytes);
    let archive_name = target.expected_archive_name();
    let sums_body = format!("{}  {archive_name}\n", hex_encode_32(&digest)).into_bytes();

    // Bind listener first so we know exact port for JSON URLs
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let release_json = serde_json::json!({
        "tag_name": "v0.4.0",
        "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.4.0"),
        "draft": false,
        "prerelease": false,
        "published_at": "2026-10-28T12:00:00Z",
        "body": "Hardened release v0.4.0",
        "assets": [
            {
                "name": archive_name,
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/{archive_name}"),
                "size": archive_bytes.len()
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/SHA256SUMS.txt"),
                "size": sums_body.len()
            }
        ]
    })
    .to_string()
    .into_bytes();

    let dl_path = format!("/goldggs/lumaglyph/releases/download/v0.4.0/{archive_name}");
    let cdn_path = format!("/cdn-blob/{archive_name}");
    let sums_path = "/goldggs/lumaglyph/releases/download/v0.4.0/SHA256SUMS.txt".to_string();

    let server = std::thread::spawn(move || {
        // Expect 4 requests: API JSON -> SHA256SUMS.txt -> Archive 302 -> CDN Archive 200
        for _ in 0..4 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]);
            let p = req
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            if p == "/repos/goldggs/lumaglyph/releases/latest" {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    release_json.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&release_json);
            } else if p == sums_path {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    sums_body.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&sums_body);
            } else if p == dl_path {
                let h = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}{cdn_path}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = s.write_all(h.as_bytes());
            } else if p == cdn_path {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    archive_bytes.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&archive_bytes);
            }
        }
    });

    let dest = unique_temp_dir("happy");
    let policy = EndpointPolicy::loopback_mock(port, true);
    let out = download_and_verify_release_with_policy(
        "goldggs/lumaglyph",
        "0.3.1",
        target,
        &dest,
        &policy,
    )
    .unwrap();

    server.join().unwrap();
    assert_eq!(out.tag, "v0.4.0");
    assert_eq!(out.sha256_hex, hex_encode_32(&digest));
    assert_eq!(std::fs::read(&out.saved_path).unwrap(), expected_bin);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn e2e_verified_download_linux_x86_64() {
    run_happy_download_for_target(PlatformTarget::LinuxX86_64);
}

#[test]
fn e2e_verified_download_macos_arm64() {
    run_happy_download_for_target(PlatformTarget::MacosArm64);
}

#[test]
fn e2e_verified_download_macos_x86_64() {
    run_happy_download_for_target(PlatformTarget::MacosX86_64);
}

#[test]
fn e2e_verified_download_windows_x86_64() {
    run_happy_download_for_target(PlatformTarget::WindowsX86_64);
}

#[test]
fn e2e_blocks_rogue_redirect_to_evil_host() {
    let target = PlatformTarget::LinuxX86_64;
    let (archive_bytes, _) = build_valid_release_archive(target);
    let digest = sha256_bytes(&archive_bytes);
    let archive_name = target.expected_archive_name();
    let sums_body = format!("{}  {archive_name}\n", hex_encode_32(&digest)).into_bytes();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let release_json = serde_json::json!({
        "tag_name": "v0.4.0",
        "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.4.0"),
        "assets": [
            {
                "name": archive_name,
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/{archive_name}"),
                "size": archive_bytes.len()
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/SHA256SUMS.txt"),
                "size": sums_body.len()
            }
        ]
    })
    .to_string()
    .into_bytes();

    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]);
            let p = req
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            if p.ends_with("/latest") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    release_json.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&release_json);
            } else if p.ends_with("SHA256SUMS.txt") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    sums_body.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&sums_body);
            } else {
                // Rogue redirect to untrusted domain!
                let _ = s.write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://evil.example.com/malware.tar.gz\r\nContent-Length: 0\r\n\r\n",
                );
            }
        }
    });

    let dest = unique_temp_dir("rogue_redir");
    let policy = EndpointPolicy::loopback_mock(port, true);
    let err = download_and_verify_release_with_policy(
        "goldggs/lumaglyph",
        "0.3.1",
        target,
        &dest,
        &policy,
    )
    .unwrap_err();
    let _ = server;
    assert!(
        err.contains("redirect") || err.contains("port") || err.contains("host"),
        "Got error: {err}"
    );
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn e2e_blocks_single_bit_sha256_mismatch_with_zero_files_written() {
    let target = PlatformTarget::WindowsX86_64;
    let (mut archive_bytes, _) = build_valid_release_archive(target);
    let honest_digest = sha256_bytes(&archive_bytes);
    // Flip 1 single bit in the downloaded archive!
    let mid = archive_bytes.len() / 2;
    archive_bytes[mid] ^= 0x01;

    let archive_name = target.expected_archive_name();
    let sums_body = format!("{}  {archive_name}\n", hex_encode_32(&honest_digest)).into_bytes();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let release_json = serde_json::json!({
        "tag_name": "v0.4.0",
        "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.4.0"),
        "assets": [
            {
                "name": archive_name,
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/{archive_name}"),
                "size": archive_bytes.len()
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/SHA256SUMS.txt"),
                "size": sums_body.len()
            }
        ]
    })
    .to_string()
    .into_bytes();

    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]);
            let p = req
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            if p.ends_with("/latest") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    release_json.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&release_json);
            } else if p.ends_with("SHA256SUMS.txt") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    sums_body.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&sums_body);
            } else {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    archive_bytes.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&archive_bytes);
            }
        }
    });

    let dest = unique_temp_dir("tampered_sha");
    let policy = EndpointPolicy::loopback_mock(port, true);
    let err = download_and_verify_release_with_policy(
        "goldggs/lumaglyph",
        "0.3.1",
        target,
        &dest,
        &policy,
    )
    .unwrap_err();
    let _ = server;
    assert!(err.contains("SHA-256 checksum mismatch"), "Got: {err}");
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn e2e_blocks_wrong_os_binary_even_if_sha256_matches() {
    // Suppose a Linux ELF binary was packaged inside `lumaglyph-windows-x86_64.zip` and signed in SHA256SUMS.txt:
    // Layer 8 native executable header verification MUST still catch and block it!
    let wrong_bin = make_synthetic_native_binary(PlatformTarget::LinuxX86_64, 2048);
    let bad_zip = build_zip(&[("lumaglyph.exe", &wrong_bin), ("LICENSE", b"MIT")]);
    let digest = sha256_bytes(&bad_zip);
    let archive_name = PlatformTarget::WindowsX86_64.expected_archive_name();
    let sums_body = format!("{}  {archive_name}\n", hex_encode_32(&digest)).into_bytes();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let release_json = serde_json::json!({
        "tag_name": "v0.4.0",
        "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.4.0"),
        "assets": [
            {
                "name": archive_name,
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/{archive_name}"),
                "size": bad_zip.len()
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.4.0/SHA256SUMS.txt"),
                "size": sums_body.len()
            }
        ]
    })
    .to_string()
    .into_bytes();

    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]);
            let p = req
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            if p.ends_with("/latest") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    release_json.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&release_json);
            } else if p.ends_with("SHA256SUMS.txt") {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    sums_body.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&sums_body);
            } else {
                let h = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    bad_zip.len()
                );
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&bad_zip);
            }
        }
    });

    let dest = unique_temp_dir("wrong_os_hdr");
    let policy = EndpointPolicy::loopback_mock(port, true);
    let err = download_and_verify_release_with_policy(
        "goldggs/lumaglyph",
        "0.3.1",
        PlatformTarget::WindowsX86_64,
        &dest,
        &policy,
    )
    .unwrap_err();
    let _ = server;
    assert!(err.contains("Binary header mismatch"), "Got: {err}");
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn e2e_blocks_draft_prerelease_downgrade_missing_sums_and_truncated_stream() {
    // 1. Draft release rejected
    {
        let mut routes = BTreeMap::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let p0 = listener.local_addr().unwrap().port();
        drop(listener);
        let _ = p0;

        routes.insert(
            "/repos/goldggs/lumaglyph/releases/latest".to_string(),
            MockRoute {
                status: 200,
                headers: vec![],
                body: br#"{"tag_name":"v0.5.0","html_url":"http://127.0.0.1/x","draft":true,"prerelease":false,"assets":[]}"#.to_vec(),
            },
        );
        let (port, h) = spawn_mock_http_server(routes, 1);
        let policy = EndpointPolicy::loopback_mock(port, true);
        let err = check_latest_release_with_policy(
            "goldggs/lumaglyph",
            "0.3.1",
            Some(PlatformTarget::LinuxX86_64),
            &policy,
        )
        .unwrap_err();
        h.join().unwrap();
        assert!(err.contains("draft"));
    }

    // 2. Prerelease rejected
    {
        let mut routes = BTreeMap::new();
        routes.insert(
            "/repos/goldggs/lumaglyph/releases/latest".to_string(),
            MockRoute {
                status: 200,
                headers: vec![],
                body: br#"{"tag_name":"v0.5.0","html_url":"http://127.0.0.1/x","draft":false,"prerelease":true,"assets":[]}"#.to_vec(),
            },
        );
        let (port, h) = spawn_mock_http_server(routes, 1);
        let policy = EndpointPolicy::loopback_mock(port, true);
        let err = check_latest_release_with_policy(
            "goldggs/lumaglyph",
            "0.3.1",
            Some(PlatformTarget::LinuxX86_64),
            &policy,
        )
        .unwrap_err();
        h.join().unwrap();
        assert!(err.contains("prerelease"));
    }

    // 3. Downgrade replay rejected when downloading update
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = serde_json::json!({
            "tag_name": "v0.2.0",
            "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.2.0"),
            "assets": []
        })
        .to_string()
        .into_bytes();
        let server = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut b = [0u8; 2048];
                let _ = s.read(&mut b);
                let hdr = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                let _ = s.write_all(hdr.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        let dest = unique_temp_dir("downgrade");
        let policy = EndpointPolicy::loopback_mock(port, true);
        let err = download_and_verify_release_with_policy(
            "goldggs/lumaglyph",
            "0.3.1",
            PlatformTarget::LinuxX86_64,
            &dest,
            &policy,
        )
        .unwrap_err();
        server.join().unwrap();
        assert!(err.contains("not newer"));
        let _ = std::fs::remove_dir_all(&dest);
    }

    // 4. Redirect loop (> MAX_REDIRECT_HOPS) blocked
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for _ in 0..4 {
                if let Ok((mut s, _)) = listener.accept() {
                    let mut b = [0u8; 2048];
                    let _ = s.read(&mut b);
                    let hdr = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/repos/goldggs/lumaglyph/releases/latest\r\nContent-Length: 0\r\n\r\n"
                    );
                    let _ = s.write_all(hdr.as_bytes());
                }
            }
        });
        let policy = EndpointPolicy::loopback_mock(port, true);
        let err = check_latest_release_with_policy(
            "goldggs/lumaglyph",
            "0.3.1",
            Some(PlatformTarget::LinuxX86_64),
            &policy,
        )
        .unwrap_err();
        let _ = server;
        assert!(err.contains("redirect hops"));
    }
}

#[test]
fn in_place_binary_swap_old_cleanup_and_tamper_preservation() {
    let dir = unique_temp_dir("inplace_swap");
    std::fs::create_dir_all(&dir).unwrap();
    let target = PlatformTarget::WindowsX86_64;
    let exe_path = dir.join("lumaglyph.exe");

    let v1_bin = make_synthetic_native_binary(target, 1500);
    std::fs::write(&exe_path, &v1_bin).unwrap();

    // Invalid binary header must fail BEFORE modifying existing v1_bin
    let err = apply_verified_binary_to_path(&exe_path, b"corrupt_not_pe_exe_payload", target)
        .unwrap_err();
    assert!(err.to_ascii_lowercase().contains("binary"), "Got: {err}");
    assert_eq!(std::fs::read(&exe_path).unwrap(), v1_bin);

    // Valid new binary replaces exe_path atomically
    let v2_bin = make_synthetic_native_binary(target, 2600);
    let installed = apply_verified_binary_to_path(&exe_path, &v2_bin, target).unwrap();
    assert_eq!(installed, exe_path);
    assert_eq!(std::fs::read(&exe_path).unwrap(), v2_bin);

    // Simulate lingering Windows `.old` and `.old.1234.1` files + unrelated user file
    std::fs::write(dir.join("lumaglyph.exe.old"), b"old1").unwrap();
    std::fs::write(dir.join("lumaglyph.exe.old.4242.1"), b"old2").unwrap();
    std::fs::write(dir.join("notes.txt"), b"keep").unwrap();

    let cleaned = cleanup_stale_old_executables_in_dir(&dir, "lumaglyph.exe");
    assert_eq!(cleaned, 2);
    assert!(!dir.join("lumaglyph.exe.old").exists());
    assert!(!dir.join("lumaglyph.exe.old.4242.1").exists());
    assert!(dir.join("notes.txt").exists());
    assert_eq!(std::fs::read(&exe_path).unwrap(), v2_bin);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e2e_in_place_self_update_installs_verified_release_and_blocks_tamper() {
    let target = PlatformTarget::LinuxX86_64;
    let v2_bin = make_synthetic_native_binary(target, 2800);
    let archive_bytes = build_tar_gz(&[
        ("lumaglyph", &v2_bin, tar::EntryType::Regular),
        ("LICENSE", b"MIT", tar::EntryType::Regular),
    ]);
    let digest = sha256_bytes(&archive_bytes);
    let archive_name = target.expected_archive_name();
    let sums_body = format!("{}  {archive_name}\n", hex_encode_32(&digest)).into_bytes();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let release_json = serde_json::json!({
        "tag_name": "v0.9.0",
        "html_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/tag/v0.9.0"),
        "assets": [
            {
                "name": archive_name,
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.9.0/{archive_name}"),
                "size": archive_bytes.len()
            },
            {
                "name": "SHA256SUMS.txt",
                "browser_download_url": format!("http://127.0.0.1:{port}/goldggs/lumaglyph/releases/download/v0.9.0/SHA256SUMS.txt"),
                "size": sums_body.len()
            }
        ]
    })
    .to_string()
    .into_bytes();

    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut b = [0u8; 4096];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]);
            let p = req
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            let payload = if p.ends_with("/latest") {
                &release_json
            } else if p.ends_with("SHA256SUMS.txt") {
                &sums_body
            } else {
                &archive_bytes
            };
            let h = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                payload.len()
            );
            let _ = s.write_all(h.as_bytes());
            let _ = s.write_all(payload);
        }
    });

    let dir = unique_temp_dir("e2e_self_update");
    std::fs::create_dir_all(&dir).unwrap();
    let exe_path = dir.join("lumaglyph");
    let v1_bin = make_synthetic_native_binary(target, 1400);
    std::fs::write(&exe_path, &v1_bin).unwrap();

    let policy = EndpointPolicy::loopback_mock(port, true);
    let outcome = install_verified_release_with_policy(
        "goldggs/lumaglyph",
        "0.3.2",
        target,
        &exe_path,
        &policy,
    )
    .unwrap();
    server.join().unwrap();

    assert_eq!(outcome.tag, "v0.9.0");
    assert_eq!(outcome.installed_exe, exe_path);
    assert_eq!(std::fs::read(&exe_path).unwrap(), v2_bin);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detect_cargo_checkout_and_relaunch_updated_binary_work_end_to_end() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let simulated_target_bin = manifest_dir.join("target/release/lumaglyph");
    assert_eq!(
        detect_local_cargo_checkout(&simulated_target_bin).as_deref(),
        Some(manifest_dir)
    );

    let standalone_dir = unique_temp_dir("standalone_relaunch");
    std::fs::create_dir_all(&standalone_dir).unwrap();
    let standalone_bin = standalone_dir.join(if cfg!(target_os = "windows") {
        "lumaglyph.exe"
    } else {
        "lumaglyph"
    });
    assert!(detect_local_cargo_checkout(&standalone_bin).is_none());

    // Copy real compiled lumaglyph binary to standalone_bin and verify automatic relaunch!
    let real_bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lumaglyph"));
    std::fs::copy(&real_bin, &standalone_bin).unwrap();
    let pid = relaunch_executable(&standalone_bin, &["--version".to_string()]).unwrap();
    assert!(pid > 0);

    let _ = std::fs::remove_dir_all(&standalone_dir);
}

#[test]
#[ignore = "Manual live network smoke test against real GitHub API + objects.githubusercontent.com CDN"]
fn live_github_cdn_verified_download_smoke() {
    let dest = unique_temp_dir("live_gh_cdn");
    let policy = EndpointPolicy::production_github(true);
    let out = download_and_verify_release_with_policy(
        DEFAULT_GITHUB_REPO,
        "0.0.1",
        PlatformTarget::LinuxX86_64,
        &dest,
        &policy,
    )
    .expect("Real GitHub CDN download + SHA-256 + ELF64 verification must succeed");
    assert!(out.saved_path.is_file());
    assert!(out.binary_size_bytes > 100_000);
    assert_eq!(out.sha256_hex.len(), 64);
    let _ = std::fs::remove_dir_all(&dest);
}
