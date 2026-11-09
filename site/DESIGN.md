---
name: cc-moss
description: A quiet keeper for your Claude Code sessions. Warm lamplight on a dark desk.
colors:
  lamp-coral: "#ff5a3c"
  lamp-coral-deep: "#d33c1d"
  lamp-coral-ink: "#c0381c"
  lamp-coral-on-tint: "#a8301a"
  ember-glow: "#ff8a6e"
  comet-coral: "#ff7a5e"
  waiting-amber: "#e6ad4d"
  night-soot: "#0c0b0a"
  banked-ash: "#141311"
  raised-ember: "#22201d"
  lamplit-paper: "#f5f0ea"
  morning-linen: "#f3efe9"
  linen-plate: "#fbf9f6"
  linen-raise: "#ffffff"
  ink-umber: "#1b1815"
typography:
  display:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "clamp(2.75rem, 7.2vw, 5.75rem)"
    fontWeight: 600
    lineHeight: 0.98
    letterSpacing: "-0.045em"
  headline:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "clamp(2rem, 4.5vw, 3.25rem)"
    fontWeight: 600
    lineHeight: 1.06
    letterSpacing: "-0.035em"
  title:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.875rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.03em"
  title-small:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.55
    letterSpacing: "-0.01em"
  lead:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 400
    lineHeight: 1.625
  body:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.625
  label:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.43
  caption:
    fontFamily: "Geist, PingFang SC, Hiragino Sans GB, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.33
  mono:
    fontFamily: "Geist Mono, ui-monospace, SF Mono, Menlo, monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  pill: "9999px"
  shell: "2rem"
  core: "26px"
  panel: "1.25rem"
  card: "1rem"
  window: "0.75rem"
spacing:
  tray: "6px"
  gutter: "20px"
  gutter-wide: "32px"
  card-pad: "40px"
  section: "80px"
  section-wide: "112px"
  section-hero: "176px"
  container: "72rem"
components:
  button-primary:
    backgroundColor: "{colors.lamp-coral-deep}"
    textColor: "#ffffff"
    rounded: "{rounded.pill}"
    padding: "6px 6px 6px 24px"
  button-copy:
    textColor: "{colors.lamplit-paper}"
    typography: "{typography.mono}"
    rounded: "{rounded.pill}"
    padding: "12px 20px"
  button-nav-install:
    backgroundColor: "{colors.lamplit-paper}"
    textColor: "{colors.night-soot}"
    rounded: "{rounded.pill}"
    padding: "8px 16px"
  nav-island:
    rounded: "{rounded.pill}"
    height: "56px"
    width: "880px"
  card-shell:
    rounded: "{rounded.shell}"
    padding: "{spacing.tray}"
  card-core:
    backgroundColor: "{colors.banked-ash}"
    rounded: "{rounded.core}"
    padding: "{spacing.card-pad}"
  eyebrow:
    textColor: "{colors.lamp-coral}"
    typography: "{typography.label}"
  keycap:
    rounded: "{rounded.card}"
    size: "96px"
---

# Design System: cc-moss

## Overview

**Creative North Star: "The Night-Shift Lamp"**

cc-moss is a lamp left on over a dark desk while the work runs through the night. The page is mostly dark and
quiet, the colour of soot and banked ash, and a single warm coral light keeps watch: it glows behind the hero,
runs the loop as a comet, marks the button you came to press, and dots the session that is waiting for you.
Nothing shouts. The light is always on, and it is the only thing that is bright.

Surfaces feel made rather than drawn: a thin tray holds an inner plate with concentric corners, keycaps press down,
buttons give under the finger with a spring. A fine film grain lies over everything so the dark reads as material,
not as an empty screen. Mockups of the real app sit on the page as physical screens and stay dark in both skins.
A light skin exists (linen and umber, the same coral) and follows the visitor's system, but the world is
conceived at night.

Rejected on purpose: the cold blue-violet gradients of generic AI products, coral spread across large surfaces,
the green-on-black developer-tool look, and fabricated social proof.

**Key Characteristics:**
- Dark, warm neutrals with one coral light; amber appears only as "waiting".
- Large, tightly tracked Geist headlines with balanced wrapping; calm grey body text.
- Machined two-layer cards (tray + plate) with hairline borders instead of heavy outlines.
- Pill-shaped controls that press in with a spring easing.
- Slow, ambient motion (float, breathe, comet) that stops for reduced-motion visitors.
- Bilingual by construction: every string exists in Chinese and English at the same weight.

## Colors

Warm near-blacks and one lamp of coral, with amber held back for the single meaning "waiting for you".

### Primary
- **Lamp Coral** (`lamp-coral`): the brand light. The main call to action, the coral eyebrow above every section
  title, the `$` in commands, "busy" status dots, the hero glow, focus rings and the newest Changelog node.
- **Ember Glow** (`ember-glow`): the softer coral for secondary emphasis next to Lamp Coral.
- **Comet Coral** (`comet-coral`): the bright head of the comet running the hero loop; used only there.
- **Lamp Coral Deep** (`lamp-coral-deep`): the fill behind white text (the primary button, the restore demo's
  button), deep enough for 4.5:1 while its glow stays Lamp Coral.
- **Lamp Coral Ink** (`lamp-coral-ink`): Lamp Coral in the light skin. On linen the lamp burns darker so coral
  words keep 4.5:1; it fills the same `--color-coral` variable, and `.dark-scope` mockups put Lamp Coral back.
- **Lamp Coral on Tint** (`lamp-coral-on-tint`, `text-coral-on-tint`): coral words sitting on a coral tint (the
  version badge, the chosen terminal chip). Lamp Coral in the dark skin, a deeper `#a8301a` on linen, always on a
  10% coral tint so the pair keeps 4.5:1.

### Secondary
- **Waiting Amber** (`waiting-amber`): means "a session is waiting for you". Status dots, the badge on the menu
  bar icon, the `?` of a question in the terminal mockup. Never decorative.

### Neutral
- **Night Soot** (`night-soot`): the page background in the dark skin, and the text of light-on-dark pills.
- **Banked Ash** (`banked-ash`): inner plates of cards (`.core`), one step above the page.
- **Raised Ember** (`raised-ember`): small floating surfaces such as the summoned search popover.
- **Lamplit Paper** (`lamplit-paper`): text in the dark skin. Secondary text never goes below two strengths,
  `text-muted` and `text-faint` (see the Readable Floor Rule); stronger text uses opacity steps above them.
- **Morning Linen** / **Linen Plate** / **Linen Raise** / **Ink Umber**: the light skin's page, plate, raised
  surface and text, mapped one to one onto the dark neutrals above.
- Hairlines, trays and drop shadows are skin-aware variables (`--hair`, `--tray`, `--drop`) rather than fixed
  colours: white at 8% / 3.5% on dark, umber at 9% / 3.5% on light.

### Named Rules
**The One Lamp Rule.** Lamp Coral is a light source, not a paint. It fills at most one control per view (the
primary button) and otherwise appears as text, dots, glows and strokes. No coral section backgrounds.

**The Amber Means Waiting Rule.** Waiting Amber is a status, not an accent. If it is on screen, something is
waiting for the visitor (or, in a mockup, for the user).

**The Readable Floor Rule.** Quiet text is quiet, never unreadable: secondary text uses `text-muted`
(60% dark, 72% light) and captions, meta and labels `text-faint` (50% dark, 64% light), both at least 4.5:1 on
every surface of their skin. Nothing that people read goes fainter than `text-faint`; the strengths are skin
variables (`--muted`, `--faint`), so a class never has to know which skin it is in.

**The Unlit Means Stopped Rule.** A stopped session is a light gone out: its dot is the text colour at 30%, and it
lights coral when it comes back. There is no red.

**The Real Screens Stay Dark Rule.** Device mockups use `.dark-scope` and keep the dark palette in both skins,
like real screens on a desk.

## Typography

**Display Font:** Geist (with PingFang SC, Hiragino Sans GB, system-ui)
**Body Font:** Geist (same stack)
**Label/Mono Font:** Geist Mono (with SF Mono, Menlo)

**Character:** One family carries everything; hierarchy comes from size, weight 600 and tight negative tracking
rather than from a second typeface. Chinese falls back to PingFang SC so both languages share the same rhythm.

### Hierarchy
- **Display** (600, clamp 2.75–5.75rem, 0.98, −0.045em): the hero line only, one per page.
- **Headline** (600, clamp 2–3.25rem, 1.06, −0.035em): section titles, always under a coral eyebrow.
- **Title** (600, 1.875–2.25rem, −0.03em): feature card titles.
- **Title Small** (600, 1.125rem, −0.01em): FAQ questions and list headings.
- **Lead** (400, 1.125–1.25rem, 1.625): the hero subtitle, `text-muted`, max width about 36rem.
- **Body** (400, 1rem, 1.625): answers and descriptions, `text-muted` or `text-faint`, `text-wrap: pretty`.
- **Label** (500, 0.875rem): the coral eyebrow above headlines; nav links in `text-muted`.
- **Caption** (400, 0.75rem): meta lines, copy labels, mockup menu bars and keycap legends; the smallest size,
  nothing goes under it.
- **Mono** (400, 13px): install commands and terminal mockups; `$` in coral.

### Named Rules
**The Balanced Headline Rule.** Headings use `text-wrap: balance` and explicit `<br>` only where the line break
carries meaning (「文明会延续，<br>对话也是。」). Chinese lines break between phrases, not inside a word:
keep a phrase together with `whitespace-nowrap` where a narrow screen would split it, and never let a full stop
fall to a line of its own.

**The Eyebrow Rule.** Every section headline is introduced by a short coral label in sentence case. The label
names the topic; the headline says something about it.

## Layout

A single centred container (72rem) with 20px side gutters, 32px from the small breakpoint. Sections breathe:
80px vertical padding on phones and 112px from `sm`, with the hero and the closing install section taller
(128–176px). Section heads either sit above the content (centred hero, left-aligned headers capped at about
42rem) or, for the FAQ, in a sticky left column (4 of 12) beside the content (8 of 12) on large screens.

Feature cards use a 12-column bento grid from `md` (7 + 5, then varied spans) with 16px gaps and collapse to one
column on phones. The tour pins a stage while steps scroll past it. The navigation is a floating island fixed
16px from the top, at most 880px wide; section links hide below `md`.

Breakpoints are Tailwind defaults: `sm` 640px, `md` 768px, `lg` 1024px.

## Elevation & Depth

Depth comes from layering and light rather than from stacked shadows. The page is the darkest layer; cards sit on
a translucent tray with a 1px inset hairline and one long, soft drop (40px 100px, offset −30px) that reads as
the card's own shadow on the desk. Inside, the plate gets a top highlight hairline. Floating elements (the nav,
popovers, the panel screenshot) add a backdrop blur or a deeper drop. Coral glow is the one "lit" elevation: it
marks the primary button, the pressed keycap and the newest release.

### Shadow Vocabulary
- **Tray** (`inset 0 0 0 1px var(--hair), 0 40px 100px -30px var(--drop)`): the outer layer of every card.
- **Plate** (`inset 0 1px 0 var(--hair), inset 0 0 0 1px var(--hair)`): the inner layer of every card.
- **Nav island** (`inset 0 0 0 1px var(--hair), 0 20px 50px -20px var(--drop)`), with `backdrop-blur-xl`.
- **Lamp glow** (`0 10px 40px -10px rgba(255,90,60,.7)`): the primary button only.
- **Keycap** (`inset 0 1px 0 var(--hair), 0 6px 0 var(--key-base), 0 12px 28px var(--drop)`): a physical key
  with its own base; pressed, it drops 5px and lights coral.

### Named Rules
**The Hairline, Not Border Rule.** Edges are 1px inset shadows in the skin's hairline colour, never solid
borders in a visible grey. Dividers inside lists use the text colour at 7%.

**The Grain Rule.** A fixed film grain (4.5% dark, 3% light) covers the whole page above content. Keep it; it is
what makes the dark feel like a material.

## Shapes

Round and machined. Interactive controls are full pills. Cards are 2rem trays holding plates whose radius is the
tray's minus its 6px padding, so the corners stay concentric. Mockup windows and screenshots use smaller,
device-like radii (0.75–1.25rem). Keycaps are rounded squares (1rem). There are no sharp corners on anything a
visitor can touch.

**The Concentric Corners Rule.** A nested surface's radius equals its parent's radius minus the gap between them.

## Components

### Buttons
Warm, weighted, pressable.
- **Shape:** full pill.
- **Primary** (`button-primary`): Lamp Coral Deep fill, white text, semibold, with an inner dark circle holding an
  arrow at the right end; lamp glow below. Hover brightens 10% and nudges the arrow up and right; press scales to
  0.98.
- **Copy command** (`button-copy`): a ghost pill (text colour at 5%, hairline inset) with the command in Geist Mono,
  a coral `$`, and a quiet "Copy" label that turns into a coral "Copied" for 1.6s.
- **Nav install** (`button-nav-install`): an inverted pill (text colour as fill, page colour as text); hover
  fades to 90%, press scales to 0.97.
- **Icon buttons:** 40px circles, `text-muted`, a 6% fill on hover, press scales to 0.9.
- **Touch size:** anything a finger can press is at least 44px tall, including segmented controls, the nav and
  footer links; links inside a sentence are the only exception.
- **One ask at a time:** while the hero's primary button is on screen, the nav's Install pill fades out (and is
  inert), so the first screen asks once.
- **Focus:** one ring everywhere, a 2px Lamp Coral outline 3px off the edge.

### Cards / Containers
- **Corner Style:** 2rem tray, 26px plate (concentric).
- **Background:** translucent tray, Banked Ash plate.
- **Shadow Strategy:** Tray + Plate from Elevation.
- **Border:** hairline inset only.
- **Internal Padding:** 32px, 40px from `sm`.

### Navigation
A floating glass island: 56px tall pill, page colour at 70% with heavy blur, hairline and soft drop. Links are
small, 55% opacity, and become full opacity with a 6% pill behind them for the section in view. On the right sit
the language segmented control (中 / EN), the skin toggle (a sun whose rays fold into a moon), GitHub, and the
inverted Install pill.

### Segmented control
A 6% pill track with small pill segments; the chosen one is lifted (text colour fill, page colour text).

### FAQ item
A disclosure row: question in Title Small with a plus icon at 40% on the right that turns 45° into a close mark
when open. One answer is open at a time; answers unfold with a height and fade transition where the browser
supports it. Rows are separated by 7% hairlines and gathered under small `text-faint` group names.

### Keycap (signature)
A physical key with a gradient top, a 6px base and a long drop. Pressed in turn to demonstrate "double-tap
Control": it sinks 5px and its glyph and outline light coral.

### The loop and Mo (signature)
The brand's infinity loop drawn huge in the hero with a coral comet circling it, and Mo, a small stone with a
face, floating 8px up and down with a lamp that breathes. They carry the personality so the controls can stay
plain.

## Do's and Don'ts

### Do:
- **Do** keep one coral-filled control per view and use Lamp Coral elsewhere as light: text, dots, glows, strokes.
- **Do** build cards as tray + plate with concentric corners and hairline insets.
- **Do** introduce each section with a coral eyebrow and a tightly tracked Geist headline.
- **Do** use the spring easing `cubic-bezier(0.32, 0.72, 0, 1)` for state changes and reveals (no overshoot), and
  stop all ambient motion under `prefers-reduced-motion`. The hero's words are there from the first frame; the
  comet on the loop is its one moving thing.
- **Do** hide things only once the script has run: reveals, the tour's dimmed steps and its stacked screens wait for
  `.js` on `<html>`, so a page whose script never loads still shows everything. On phones the tour's captions all
  stay readable; only wide screens dim the steps you're not on.
- **Do** show the real app (screenshots per language and skin) inside dark device mockups.
- **Do** give every string both languages at equal weight, and check both skins.

### Don't:
- **Don't** use cold blue-violet gradients or the generic "AI product" look.
- **Don't** paint large areas in coral or use it as a section background.
- **Don't** make the page look like a developer tool: no green-on-black terminals as the main visual, no dense
  parameter tables; the terminal appears only as a scene prop.
- **Don't** add logo walls, star ratings, user counts or other social proof that does not exist.
- **Don't** use amber for anything other than "waiting".
- **Don't** draw solid grey borders; use hairline insets.
