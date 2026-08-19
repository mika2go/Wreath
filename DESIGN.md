---
name: Wreath — The Quiet Replay Deck
description: A calm monochrome Windows workspace for capturing, finding, organizing, and trimming local clips.
colors:
  canvas-black: "#0b0b0c"
  video-stage: "#101011"
  surface: "#121214"
  surface-raised: "#18181a"
  surface-hover: "#222225"
  contour: "#2d2d30"
  text-primary: "#f2f2f3"
  text-secondary: "#b4b4b8"
  action-light: "#edeef2"
  action-light-hover: "#ffffff"
  selection: "#424854"
  ready: "#35d07f"
  warning: "#f0b849"
  danger: "#f15b68"
typography:
  display:
    fontFamily: "Segoe UI Variable Display, Segoe UI, sans-serif"
    fontSize: "31px"
    fontWeight: 600
    lineHeight: 1.2
  headline:
    fontFamily: "Segoe UI Variable Display, Segoe UI, sans-serif"
    fontSize: "29px"
    fontWeight: 600
    lineHeight: 1.25
  title:
    fontFamily: "Segoe UI Variable Display, Segoe UI, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.35
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.4
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.4
rounded:
  compact: "6px"
  control: "8px"
  surface: "10px"
  modal: "12px"
spacing:
  xs: "6px"
  sm: "10px"
  md: "14px"
  lg: "18px"
  xl: "24px"
  page: "40px"
components:
  button-primary:
    backgroundColor: "{colors.action-light}"
    textColor: "{colors.canvas-black}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 20px"
    height: "44px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 18px"
    height: "44px"
  field:
    backgroundColor: "{colors.video-stage}"
    textColor: "{colors.text-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "40px"
  surface-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.surface}"
    padding: "20px"
---

# Design System: Wreath — The Quiet Replay Deck

## Overview

**Creative North Star: "The Quiet Replay Deck"**

Wreath is a restrained native Windows control surface: nearly black, compact, and immediately operational. The UI recedes behind the user's recordings; hierarchy comes from spacing, typography, thin contours, and real clip imagery rather than decorative effects.

The product uses the normal immersive-dark Windows titlebar. The client area is rendered with Direct2D and DirectWrite, remains monochrome except for state and destructive colors, and preserves native Windows behaviors for moving, maximizing, minimizing, and closing the window.

**Key Characteristics:**

- Native dark Windows frame with a calm monochrome Direct2D client.
- Dense, scan-first layouts with restrained 1px contours and rounded corners.
- Global clips, collection-specific clips, settings, preview, and editing remain visibly distinct workflows.
- Hover and selection clarify existing actions; they do not rearrange content.

## Colors

The palette is neutral charcoal and warm white; color is reserved for readiness, warnings, selection, and destructive actions.

### Primary

- **Action Light:** Used for the highest-priority button, active trim boundary, and strongest focus or selection contrast.

### Neutral

- **Canvas Black:** The application and sidebar ground.
- **Video Stage:** The inset ground behind video, fields, and dark secondary controls.
- **Surface:** Default cards, panels, menus, and button surfaces.
- **Raised Surface:** Active or selected surfaces that need one tonal step of separation.
- **Hover Surface:** Temporary hover and track treatment.
- **Contour:** One-pixel dividers and boundaries.
- **Primary Text:** Titles, actionable labels, and high-value metadata.
- **Secondary Text:** Descriptions, timestamps, sizes, and inactive labels.

### Tertiary

- **Ready:** Successful recorder and drag-target state only.
- **Warning:** Attention states such as incomplete hotkey capture.
- **Danger:** Delete actions and irreversible confirmations.

**The Sparse Color Rule.** Chromatic color communicates state; it is never used as page decoration.

## Typography

**Display Font:** Segoe UI Variable Display (with Segoe UI fallback)  
**Body Font:** Segoe UI Variable Text (with Segoe UI fallback)

**Character:** Native, compact, and highly legible. Semibold display faces establish hierarchy while body and metadata remain normal-weight and quiet.

### Hierarchy

- **Display** (600, 31px, 1.2): Rare large status or legacy hero titles.
- **Headline** (600, 29px, 1.25): Page titles such as Clips, Collections, Einstellungen, and Clip-Preview.
- **Title** (600, 17px, 1.35): Section headings, card values, and collection names.
- **Body** (400, 14px, 1.4): Navigation, buttons, settings labels, and primary metadata.
- **Label** (400, 12px, 1.4): Descriptions, helper text, timestamps, counts, and table headers.

**The Single-Line Control Rule.** Interactive labels and field values do not wrap; constrain them to their bounds and ellipsize variable data when needed.

## Layout

The expanded shell uses a 244px sidebar and a 40px content inset; constrained windows collapse the sidebar to 82px and reduce page inset to 28px. Page headings begin near the top of the client area below the native titlebar. Content grids use 14–18px gaps and adapt from four columns down to one based on available width.

The sidebar order is fixed: Dashboard, Clips, Collections; divider; Ordner öffnen; storage usage near the bottom; divider; Einstellungen as the bottom-most destination. Settings use a responsive two-by-two panel matrix for Allgemein, Aufnahmen, Audio, and Speicher und Qualität.

The Dashboard exposes four equal quick-setup panels for Replay-Dauer, Bildschirm, Qualität, and Audio. Each card opens its control directly; it is never hover-dependent. Collections show Alle Clips plus at most two user collections per page, followed by explicit pagination. This bounded row prevents collection cards from colliding with the clip table.

The global Clips page always represents the complete clip library, independent of the active collection. A selected collection filters only the table within Collections; choosing Alle Clips clears that collection scope and restores every clip.

## Elevation & Depth

The system is flat and layered, with no decorative shadows. Depth is expressed through small tonal changes between canvas, stage, surface, raised surface, and hover surface, reinforced by one-pixel contours.

**The Contour Before Shadow Rule.** Use a tonal step and a 1px border before adding elevation; ordinary panels and cards do not cast shadows.

## Shapes

Controls and compact fields use 6–8px corners. Cards and panels use 9–10px corners; modals and major status containers may use 12px. Clip previews inherit the card silhouette and clip their media to the upper corners. Circular shapes are reserved for toggles, progress handles, and the wreath mark.

## Components

### Buttons

- **Shape:** Compact rounded rectangles with an 8px radius and a 44px primary height.
- **Primary:** Action Light background with Canvas Black text for one dominant action per region.
- **Secondary:** Surface or Video Stage background, Primary Text, and a one-pixel Contour border.
- **Hover / Focus:** Lighten the surface or contour without changing geometry; primary buttons approach Action Light Hover.
- **Danger:** Use Danger for delete labels and confirmation treatment, never for ordinary navigation.

### Cards / Containers

- **Corner Style:** Gently rounded (9–10px).
- **Background:** Surface at rest; Raised Surface when selected.
- **Border:** One-pixel Contour; active collection cards use a stronger Primary Text contour.
- **Internal Padding:** Usually 16–22px, with 14–18px between sibling cards.
- **Behavior:** Collection cards are thinner than clip cards and widen to fill the row; their titles and descriptions remain clipped to the available card width.

### Inputs / Fields

- **Style:** Video Stage fill, one-pixel Contour, 7–9px corners.
- **Insets:** Variable settings values begin at least 10–12px inside the left edge. Dropdowns preserve a separate right inset for their chevron.
- **Overflow:** Paths and device names are ellipsized before drawing and are additionally clipped by DirectWrite to the field bounds.
- **Focus:** Promote the contour to Primary Text without adding glow or changing field size.

### Navigation

- **Style:** Line icons with 14px labels; active destinations use Raised Surface and Primary Text.
- **Hierarchy:** Dashboard, Clips, and Collections form the primary group. Ordner öffnen is separated by a divider; Einstellungen is pinned below a final divider.
- **Native frame:** Windows owns the titlebar and its standard minimize, maximize, close, drag, and system-menu behavior.

### Quick Setup Panels

The four Dashboard panels expose current values and open the corresponding settings choice on click. Their value is the focal line; the description remains secondary. A chevron signals direct configuration without requiring navigation to Einstellungen.

### Clip Preview and Editor

Normal preview keeps the application shell, page heading, Back and Clip bearbeiten actions, a fitted video stage with transport controls, and right-side information/options panels when width permits. Fullscreen removes the shell and places a black top toolbar and bottom transport/metadata surface around the video. The editor keeps the shell, adds save/discard controls, clip information, trim destination choices, and a storyboard timeline with explicit start, end, and reset actions.

## Do's and Don'ts

### Do:

- **Do** keep the native Windows dark titlebar and standard window behavior.
- **Do** preserve the sidebar hierarchy and keep Einstellungen at the bottom.
- **Do** paginate collection cards as Alle Clips plus no more than two user collections per page.
- **Do** keep the Clips library global and collection filtering local to Collections.
- **Do** clip and ellipsize paths, device names, collection titles, and other variable strings inside their controls.
- **Do** preserve the normal-preview, fullscreen-preview, and editor compositions as three distinct operating modes.

### Don't:

- **Don't** draw custom minimize, maximize, or close controls in the Direct2D client.
- **Don't** use nested decorative cards, gradients, glows, or ambient drop shadows.
- **Don't** make quick setup actions appear only on hover.
- **Don't** let collection pagination push the clip table downward or off-screen.
- **Don't** carry an active collection filter into the global Clips page.
- **Don't** let variable field content touch a border, overlap a chevron, or render outside its control.
