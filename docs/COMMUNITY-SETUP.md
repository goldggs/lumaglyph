# Setting up the community side

A checklist for turning on Issues and Discussions on the LumaGlyph repository, in the
order that avoids rework. Work through it a phase at a time; each phase is a few minutes
of clicking and nothing here can break the code.

You need to be signed in as the repository owner. All paths below start at
<https://github.com/goldggs/lumaglyph>.

---

## The difference, in one table

| | Issues | Discussions |
| :--- | :--- | :--- |
| Shape | A list of items, each open or closed | A forum of threads that stay open |
| Good for | Bugs, wrong docs, broken behaviour, specific requests | Questions, showing what you made, open-ended ideas, announcements |
| Ends when | Someone fixes it and closes it | It goes quiet |
| Shows in your queue | Yes, as a count you feel obliged to reach zero | No |
| Can host a poll | No | Yes |
| Answerable | Yes, as a Q&A category | Yes, if the category type is Q&A |

**The rule:** definite answer or fix means Issue. Conversation means Discussion.

Getting this wrong is how small projects end up with two hundred open issues that are
all people asking how a slider works.

---

## Phase 1 - Turn the two features on

1. Go to the repository page.
2. Click **Settings** in the top navigation bar (the gear, far right of the repo tabs).
3. Stay on **General**. Scroll down to the **Features** section.
4. Tick **Issues** if it is not ticked.
5. Tick **Discussions**. A **Set up discussions** button appears next to it.
6. Click **Set up discussions**, then **Start discussion** on the page it opens. This
   creates the first welcome thread and finishes the setup.
7. Go back to **Settings > General > Features** and confirm both boxes are now ticked.

That is it. Issues and Discussions are now both live on the repository.

**Do not** delete the first discussion GitHub creates. Edit it instead - it is the
welcome post people will see first.

---

## Phase 2 - Set the discussion categories

Discussions ship with a default set that is close to right. Check them, do not rebuild
them.

1. Click the **Discussions** tab.
2. Next to a category name there is a pencil icon. Use it to review each one.
3. The four that matter for this project:

| Category | Format | Use |
| :--- | :--- | :--- |
| **Announcements** | Announcement | Release notes and news. You post, nobody else does. |
| **Q&A** | Question and answer | "How do I do X". The format lets an answer be marked, which is the point. |
| **Ideas** | Open-ended discussion | Feature requests that are not yet a decision. |
| **Show and tell** | Open-ended discussion | People posting what they made with it. This is the one that builds a community. |

4. Delete or rename any default category that does not fit, rather than leaving an empty
   one that nobody uses.
5. In each category, set a short description. One line, plain words. It tells a newcomer
   where to put their post.

Suggested descriptions:

- **Announcements** - "Release notes and project news. Maintainer posts only."
- **Q&A** - "How do I do something, or why is it doing that. Mark the answer if you find it."
- **Ideas** - "Feature ideas and changes. Nothing here is promised."
- **Show and tell** - "What you made with it. Screenshots welcome."

---

## Phase 3 - Protect `main`

Do this before you invite anyone, so the rule is already in place rather than a decision
you have to make under pressure.

1. **Settings > Rules > Rulesets** (on older layouts: **Settings > Branches**).
2. **New ruleset > New branch ruleset**.
3. Name it `main protection`.
4. **Target branches**: add `main`.
5. Enable these:

| Rule | Why |
| :--- | :--- |
| **Block force pushes** | A force push on `main` rewrites history and breaks everyone's clone. |
| **Restrict deletions** | Stops `main` from being deleted by accident or by a bad script. |
| **Require status checks to pass** | The CI job must be green before a change lands. |

6. Leave **Require a pull request before merging** OFF for now, and read the note below.

### The one that needs a decision

Requiring a pull request before merging means **nobody** can push straight to `main`,
including you and including the automated release pushes this project uses. If you turn
it on, the workflow becomes: push a branch, open a PR, let CI run, merge.

That is strictly better practice, and it is what a project with outside contributors
should do eventually. It is also a real change to how releases get made.

Two honest options:

- **Stay as you are** (recommended for now): force-push and deletion blocked, CI required,
  direct pushes still allowed for you. Keep it until there are outside contributors.
- **Go full PR flow**: everything above plus "Require a pull request before merging", with
  comments on. Releases then go through a PR that has to pass CI first.

Nothing breaks either way. Pick the second when the project has someone else committing.

---

## Phase 4 - Approve workflows for first-time contributors

This is the one that protects you from a stranger running code in your CI.

1. **Settings > Actions > General**.
2. Scroll to **Fork pull request workflows**.
3. Make sure **Require approval for first-time contributors** is selected.

That setting means a PR from an account that has never contributed cannot run your
workflows until you approve it. Without it, a malicious PR can execute code on your
runner.

While you are there, leave **Workflow permissions** on the read-only default unless
something needs otherwise.

---

## Phase 5 - A small set of labels

Labels are how you sort a growing list without reading it. Six is enough; forty is
decoration.

1. Click **Issues**, then **Labels**, then **New label**.

| Label | Colour | Meaning |
| :--- | :--- | :--- |
| `bug` | `#d73a4a` | Something is wrong. |
| `documentation` | `#0075ca` | The docs are wrong or missing. |
| `good first issue` | `#7057ff` | Small, self-contained, safe for a newcomer. |
| `question` | `#d876e3` | Needs an answer before it is anything else. |
| `needs reproduction` | `#fbca04` | Cannot act until someone can reproduce it. |
| `wontfix` | `#ffffff` | Considered and declined, with a reason in the thread. |

Delete the default labels you will never use, so the list stays readable.

---

## Phase 6 - Issue templates

This is the part that connects to the Report a Problem feature, and it is worth doing
before that feature is built, not after.

Templates live in `.github/ISSUE_TEMPLATE/`. There are two kinds of file:

- **`bug_report.yml`** and friends: forms a reporter fills in. They can mark fields as
  required, so a bug report cannot arrive with no version or no platform.
- **`config.yml`**: controls the chooser page, and can add **contact links**. A contact
  link points somewhere else - Discussions, a docs page - so that people who are about to
  file the wrong thing are redirected before they do.

Done properly, the chooser looks like this:

```
[ Bug report ]  [ Documentation problem ]  [ Feature idea ]
  -> ask a question on Discussions
  -> read the troubleshooting guide
```

That last pair are contact links, and they will absorb the majority of first-time
visitors who would otherwise open an issue saying "it doesn't work".

Once the templates exist, the in-app **Report a Problem** button points at the bug
template URL with the fields prefilled:

```
https://github.com/goldggs/lumaglyph/issues/new?template=bug_report.yml&title=...&body=...
```

So: templates first, button second.

---

## Phase 7 - Maintainer hygiene

Small habits that decide whether a community feels alive:

- **Reply within a few days, even if the reply is "I have seen this and will look at it
  on the weekend".** Silence reads as abandonment faster than a slow answer does.
- **Close issues with a reason.** "Fixed in v0.4.7" or "declining because..." - not just
  a click.
- **Mark the answer in Q&A.** It saves the next person the same question.
- **Never let an issue sit with no label.** A label is a promise that it has been read.
- **Do not argue with a hostile report.** Close it, say nothing, move on. A Code of
  Conduct exists so that this is a policy rather than a mood.

---

## Order of operations

1. Phase 1 - turn on Issues and Discussions
2. Phase 2 - four categories with descriptions
3. Phase 5 - six labels
4. Phase 4 - require approval for first-time contributors
5. Phase 3 - protect `main`
6. Phase 6 - issue templates, then the in-app button

Phases 3 and 4 are the security-relevant ones; phases 1 and 2 are what makes the
repository feel like a place rather than a download page.
