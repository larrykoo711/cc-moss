<p align="center">
  <img src="docs/assets/banner.svg" alt="cc-moss — Civilizations carry on. So do conversations." width="100%">
</p>

<p align="center">
  <a href="README.md">简体中文</a> · <b>English</b>
</p>

<p align="center">
  <a href="https://github.com/larrykoo711/cc-moss/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/larrykoo711/cc-moss?style=flat-square&color=FF5A3C"></a>
  <img alt="macOS · Linux" src="https://img.shields.io/badge/macOS%20%C2%B7%20Linux-71717a?style=flat-square">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-3b82f6?style=flat-square"></a>
</p>

---

The laptop reboots. The editor crashes. A terminal tab closes when you didn't mean it to.

And the Claude Code sessions you had open are gone. Which one was fixing the login page? Which one was halfway
through a bug, waiting for your answer? You dig through transcripts and try to remember.

**cc-moss is a small tool for exactly that moment.** It quietly keeps track of every Claude Code session you start.
When something goes wrong, one command brings them all back: same folder, same conversation, right where you left off.

<p align="center">
  <img src="docs/assets/picker.svg" alt="The cc-moss session panel" width="92%">
</p>

## What it does for you

- **Nothing to remember.** Use Claude Code as you always do; cc-moss keeps the list for you.
- **One command after a crash.** Run `cc-moss restore` after a crash or a reboot, and every session picks up where
  the conversation stopped.
- **Every session at a glance.** Type `t` and they are all there, grouped by project: which one is working, which one
  is waiting for you, which one got cut off.
- **A note for later.** Jot down what a session is for and where it stopped, so tomorrow you know where to start.

No shortcuts to learn: type to search, arrows to choose, Enter to go in. It speaks English and Chinese, following your
system language.

## Three steps to set up

```sh
brew install larrykoo711/tap/cc-moss
cc-moss install
cc-moss doctor
```

`install` shows you every change before it makes it, and only goes ahead when you say yes. It **adds** to your
configuration and never overwrites what you have; every file it touches is backed up first. If a tool is missing
(tmux, say), it tells you and offers to install it. `doctor` then checks each piece and tells you how to fix anything
that is off.

You need a Mac or a Linux machine with [Claude Code](https://code.claude.com/docs). No Homebrew? You can also
[download a release or build from source](docs/guide.en.md#install).

## Everyday use

Three things cover almost everything:

| You want to… | Do this |
|---|---|
| See all your sessions | type `t` |
| Start a new one | `cc-moss new <name>`, for example `cc-moss new login-page` |
| Get everything back after a crash | `cc-moss restore` |

<table>
  <tr>
    <td width="50%"><img src="docs/assets/picker-restore.svg" alt="A session that got cut off"></td>
    <td width="50%"><img src="docs/assets/picker-menu.svg" alt="The session menu"></td>
  </tr>
  <tr>
    <td align="center"><sub>A session that got cut off keeps your note and its last message; Enter picks it back up</sub></td>
    <td align="center"><sub>→ opens the menu: open, write a note, end the session</sub></td>
  </tr>
</table>

When a new version is out, the panel says so at the top; choose it and press Enter to update.

## Your data

Everything stays on your own machine: nothing is uploaded, and there is no tracking. The only time cc-moss goes
online is to ask GitHub, at most once a day, whether there is a new version. That request says nothing about you, and
you can [turn it off](docs/guide.en.md#privacy).

## Changed your mind?

```sh
cc-moss install --uninstall
brew uninstall cc-moss
```

The first command takes out everything cc-moss added and puts your files back; the second removes the program itself.

## Learn more

- [The guide](docs/guide.en.md): every command and setting, how it works, and how it compares with other approaches
- [FAQ](docs/guide.en.md#faq): `t` does nothing after installing? Using bash? It's in there
- [Changelog](CHANGELOG.md)
- Want to help make it better? Start with [CONTRIBUTING](CONTRIBUTING.md)

## Why "MOSS"?

MOSS is the AI in *The Wandering Earth* whose mission is to keep human civilization going. cc-moss has a much smaller
mission: whatever happens to your computer, keep your conversations with Claude going.

---

<sub>Open source under [MIT](LICENSE). cc-moss is an independent project, not affiliated with Anthropic.</sub>
