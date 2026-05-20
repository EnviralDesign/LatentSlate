# egui UI/UX Rebuild Plan

This is the agreed visual-regression checklist and implementation prompt for rebuilding the egui UI against the preserved legacy UI screenshots.

## Exact Reviewed Checklist

I compared the preserved legacy screenshots against `.tmp/desktop-smoke/egui-reference-ready`. On my checkout the old screenshot folder was renamed during cleanup to `.tmp/desktop-smoke/legacy-ui-reference-20260519-173555`, but the images are the same set.

**Overall Verdict**
The current egui version is functionally useful, but visually it regressed from “custom dark creative editor” to “raw egui debug shell.” The biggest issue is not one color or one widget. It is that the legacy UI had designed surfaces, hierarchy, spacing, modal treatment, and domain-specific controls, while the egui shell currently exposes stock widgets with minimal styling.

**Priority Checklist**
1. **Create a UI foundation layer first**
   - Regression: legacy had consistent dark surfaces, section cards, input styling, green primary actions, muted labels, and controlled spacing.
   - Current: colors exist as constants, but widgets still look stock and inconsistent.
   - Fix: add reusable egui UI kit helpers: `Theme`, `UiTokens`, `PrimaryButton`, `SecondaryButton`, `IconButton`, `SectionHeader`, `FieldLabel`, `TextFieldRow`, `InspectorCard`, `ModalFrame`.

2. **Rebuild modal styling**
   - Regression: legacy startup/settings modals had polished card framing, generous padding, strong title/copy hierarchy, and clear primary/secondary actions.
   - Current: egui windows are small, stock, cramped, with default title bars and weak action hierarchy.
   - Fix: create a reusable custom modal template with backdrop/scrim, custom header, body padding, footer action bar, fixed min widths, and styled close affordance.

3. **Startup/New Project flow**
   - Regression: legacy startup was a first-class centered experience with left “Create New Project” and right “Recent Projects.”
   - Current: startup appears as a plain floating tool window over the full editor; fields are cramped and visually underspecified.
   - Fix: make `ProjectWizardModal` template: hero header, two-column layout, preset chips for resolution/aspect, grouped numeric fields, location row, large green primary CTA, recent project list card.

4. **Top bar and app chrome**
   - Regression: legacy top bar felt like app chrome: quiet menus, centered project title, compact queue pill.
   - Current: menu buttons look stock, spacing is compressed, queue indicator is visually noisy.
   - Fix: build `AppMenuBar` with styled text menus, right-side project status cluster, queue pill, consistent hover/active states.

5. **Panels**
   - Regression: legacy Assets and Attributes panels had clearer headers, better internal spacing, card-like sections, and stronger empty/selection states.
   - Current: panels are flat, dense, and labels/fields compete visually.
   - Fix: create reusable `DockPanel`, `PanelHeader`, `PanelSection`, `EmptyState`, `InspectorGroup`.

6. **Assets panel**
   - Regression: legacy asset rows had left accent, icon/thumbnail treatment, selected state, and better list rhythm.
   - Current: rows are stock selected buttons with emoji/icons, weak hierarchy, and cramped generative buttons.
   - Fix: build `AssetListRow`, `MediaTypePill`, `CreateAssetButton`, thumbnail placeholder slots, selected/hover/accent states.

7. **Attributes inspector**
   - Regression: legacy inspector grouped Clip, Transform, Timing, Marker, Track fields into clear sections with readable labels.
   - Current: fields are inline egui drag values with prefixes, cramped spacing, little grouping, and weak affordance.
   - Fix: build `InspectorFieldGrid`, `NumericField`, `SliderField`, `ColorSwatchField`, `DangerButton`, and section cards.

8. **Timeline**
   - Regression: legacy timeline had stronger editor structure: toolbar, zoom/frame controls, ruler rhythm, track labels, row separation, clip selection clarity.
   - Current: timeline is functional but flatter; clip text is tiny, selected clip styling is harsh, ruler/grid feels coarse, controls are stock.
   - Fix: create a dedicated timeline rendering kit: `TimelineToolbar`, `TimelineRuler`, `TrackHeader`, `TrackLane`, `ClipBlock`, `MarkerGlyph`, `Playhead`. Use reusable paints/tokens instead of scattered raw painter calls.

9. **Preview panel**
   - Regression: legacy preview had a framed canvas with better header/status balance.
   - Current: preview is too stark and grid/outline treatment feels accidental.
   - Fix: build `PreviewViewport` with plate border, canvas background, subtle header metadata, stats overlay panel, and empty-state treatment.

10. **Overlays/popovers**
   - Regression: legacy queue and generative modal had dimmed/blurred background treatment and felt intentionally layered.
   - Current: queue is a tiny stock floating window; modals lack consistent backdrop and focus treatment.
   - Fix: reusable `OverlayLayer` plus `PopoverPanel` for queue, menus, and lightweight floating surfaces.

11. **Provider modal**
   - Regression: legacy Providers modal had a structured list/detail split with comfortable proportions.
   - Current: modal is larger but less designed; list rows and JSON detail area feel raw.
   - Fix: build `SplitModal`, `ProviderListRow`, `JsonPreviewPane`, empty detail state, and future builder action bar.

12. **Collapsed panel states**
   - Regression: legacy collapse controls were subtle and integrated into panel chrome.
   - Current: collapsed panels show plain “A” buttons and lose context.
   - Fix: `CollapsedRail` component with vertical label/icon, tooltip, hover reveal, and consistent rail width.

13. **Typography**
   - Regression: legacy had better contrast between headings, labels, values, and helper text.
   - Current: most text is tiny, same-weight, and too muted.
   - Fix: define type roles: `title`, `section`, `label`, `body`, `caption`, `mono`. Apply consistently.

14. **Iconography**
   - Regression: legacy icons felt more controlled; current emoji symbols look inconsistent and unprofessional.
   - Fix: replace emoji/text glyphs with a small internal icon paint set or egui-compatible icon font strategy. Use consistent 12/14/16px icon sizing.

15. **Color and affordance**
   - Regression: legacy green primary action was clear; type colors and accents had stronger intent.
   - Current: green/blue/orange are overused in small widgets without a full semantic system.
   - Fix: semantic tokens: `accent.primary`, `accent.video`, `accent.audio`, `accent.marker`, `danger`, `surface.raised`, `surface.sunken`, `border.focus`.

**Implementation Order I Recommend**
1. Build the egui UI kit/tokens in one place before touching individual screens.
2. Apply it to modals first: startup, new project, project settings, generative video.
3. Rework the dock panels and inspector fields.
4. Rebuild timeline visuals as reusable painter components.
5. Polish preview, queue, providers, and collapsed rails.
6. Run the automation reference capture after each major pass and compare against the legacy anchor.

The main design-system bet: avoid patching raw egui widgets inline. We should create a small app-specific egui component layer now, because modals, inspector fields, asset rows, timeline controls, and provider-builder screens are going to repeat these same patterns constantly.
