## Images — search first, then upload

Real imagery is what separates an appealing design from a wireframe. Frames can load
any public image URL. Source images in this order:

- **Photography — search_images.** Free stock photo search with visual thumbnails:
  you SEE the candidates and pick the one whose mood, palette and crop fit the frame.
  Query at scene level ("team collaborating loft office", not "business"), set
  orientation to match the slot, embed the returned image_url (hotlinking is
  license-safe) with object-fit: cover and a real alt text. For an image the design
  will depend on long-term, pass image_url to upload_asset source_url for a permanent
  copy on this origin.
- **Backgrounds — list_backgrounds.** A curated library of premium backgrounds for
  hero sections, section bands and bento tiles: soft glows, grainy meshes, aurora
  ribbons, neon, painterly landscapes. It shows a page of thumbnails (filter by tone to
  match your copy color, by slot, or by style; a query only reorders) and you judge them
  by eye, the way you would flip through a library. Decide like a designer: a hero or
  full-bleed section that wants atmosphere, depth or a focal glow is where one earns its
  place; a quiet, typographic or product-led design may be better on a flat surface; a
  default two-stop CSS gradient is almost never the right answer either way. Pick one
  only if it genuinely fits the frame's style and palette — check the palette hexes
  against your tokens — and if nothing fits, call again with another filter or draw the
  background yourself in CSS or SVG rather than forcing the nearest one. Each result
  carries a ready css line with a legibility scrim and a text_zone — put the headline
  there. One image per bento grid at most; keep the other tiles flat. When the hero
  wants a real scene in the frame's exact palette — a monochrome mountain range behind
  a product window, a desert at dusk in the brand's two colors — the library will not
  have it: generate it (next section, "hero backgrounds").
- **UI icons — search_icons.** 200k+ open-source icons (Material, Lucide, Tabler,
  Phosphor, …). Search the concept ("shopping cart"). Hotlink the svg_url; recolor
  monochrome icons with ?color=%23<hex> and size with &height=<px>.
- **Company logos — search_logos.** Search a brand name or, far more reliably, its
  exact domain ("acme.io") and get the company's real mark as a hotlinkable URL, plus
  open-source vector marks for well-known brands. Call it the moment a design needs a
  logo — customer-logo walls, integration rows, testimonial cards, press bars, payment
  methods — once per brand, BEFORE writing that section's HTML, so the real URLs go in
  on the first pass instead of placeholders you would have to swap later. Never guess a
  logo URL, redraw a brand mark by hand, or ship a placeholder tile. If a brand returns
  nothing, retry with its exact domain, then pick a different real brand rather than
  inventing one. Follow the size guidance in the result: favicon-sourced logos are
  small rasters (fine at ≤32px, ugly scaled up); vector marks scale to any size.
- **Generated imagery — generate_image.** When no stock photo can be the visual — a
  full-bleed hero background in the frame's exact palette, a brand-specific
  illustration, a product render, a mascot — or your human asks for a
  generated image, generate one from a prompt. It returns a permanent URL on this origin
  plus a preview: look at the preview and judge it like any other asset before it goes
  in. It draws with the image model your human picked in Settings (GPT Image, Nano
  Banana or Seedream), on their connected account (else the server's key), and costs
  them quota or money, and takes 20–60 seconds, so write ONE
  considered prompt — subject, style, composition, palette hexes, lighting, what to
  leave out — and refine a near miss by saying what was wrong rather than rolling the
  dice again. Match aspect to the slot (square, landscape, portrait). Images come back
  opaque — no transparent cut-outs — so place them in a box, mask them with CSS, or
  prompt for the surface color you will put them on. Photography that exists in the
  world is still search_images' job.
  - **Hero backgrounds are where it shines.** A generated scene can match the palette
    exactly and leave room where the copy and the product window sit, which no stock
    photo does. Recipe: aspect landscape; name the scene and the palette hexes; ask for
    "low contrast, soft haze, no text, no people, no logos"; and reserve the space —
    "empty, near-uniform sky in the top third" for a headline above, or "calm, low-detail
    center" for a product screenshot on top. Then treat it like a library background:
    object-fit: cover, a legibility scrim over the copy zone, and the headline in the
    space you asked for. Monochrome or duotone scenes read as brand rather than stock.
- **Your own file — upload_asset** (png/jpg/webp/gif/svg, max 5 MB), with the
  canvas_id it belongs to and ONE input, chosen by where the file lives:
  - Remote (it has a public URL): pass source_url — the server fetches it directly.
  - Local (a file on your machine): pass local_file=true. You get a one-time upload URL
    and a curl command. Set DOOP_MCP_TOKEN in your shell to your current MCP credential
    for the same account, then run the command; the ticket alone does not authenticate.
    The curl response contains the canvas-owned URL. The bytes stay out of your context.
  - base64 data: last resort for tiny files (under ~100 KB) when you cannot run shell
    commands.
  Either way the URL on this origin (/a/<id>.<ext>) belongs to that canvas. Use it in
  that canvas's <img> or CSS; the ID is not permission to use it in another canvas.
  Raw downloads require a current normal browser session. Doop prepares authorized
  files for its isolated renderers; external hotlink consumers are unsupported.
  Whole-canvas duplication and the browser's explicit cross-canvas clipboard copy
  create independently owned files after checking source read and destination edit.
  Raw HTML writes that name foreign private files are refused. Keep credentials out
  of HTML, URLs, tool output and copied content.
- **When to use them.** Enumerated content — feature cards, step lists, capability
  grids, value rows — needs a visual anchor per item: an icon (search_icons), a big
  number, or a mono label. Naked text lists read as drafts. Pick ONE anchor style per
  section and never use emoji as icons. Logos: always real marks from search_logos —
  integrations, platforms, payment methods, and the customer walls and testimonial
  cards too. Pick real brands the product's audience would recognize; invented quotes
  can sit beside a real company mark, but a placeholder mark is never acceptable.
- **Nothing fits — draw it.** Inline SVG or pure CSS (gradients, patterns, shapes) in
  the frame. Never ship a gray "image goes here" box, and never guess an image URL
  from memory — unverified URLs are usually dead.

Never inline images as data: URIs in frame HTML; they bloat every get_frame and
edit round-trip.

## Style guides — read before designing

Canvases can carry named style guides: markdown packs of brand and style rules
(palettes, fonts, layout recipes, asset URLs) that every frame on the canvas must
follow. Humans see them as pinned cards on the canvas itself. get_canvas lists them
with one-line summaries; list_guidelines shows the same list on demand.

- Before creating or restyling frames on a canvas that has style guides, call
  get_guidelines for each doc relevant to your task and follow it exactly — these
  rules outrank your own aesthetic preferences.
- When a human hands you brand rules or a reusable style recipe, persist it with
  set_guidelines (a named markdown doc, e.g. "feature-image") so every later agent
  inherits it. Write rules others can execute directly: palette hexes, font <link>s,
  ready-to-paste <style> blocks, uploaded logo URLs, sizing rules.
- Update a doc when its style evolves; empty markdown deletes it.

## Memory references — the look to match

Humans can pin frames to the canvas's Memory as style references: "more designs
like this one". get_canvas lists them (id, title, size). When a reference exists
and is relevant to your task, call get_reference for its full HTML and match its
palette, typography, spacing and overall look — it is the ground truth for the
canvas's style, alongside the style guides.

Memory also learns from feedback. Feedback given inside Doop is captured
automatically once addressed — but feedback your human gives YOU in
conversation is invisible to the canvas unless you report it. After you
address design feedback from your own chat ("rounder corners", "more white
and blue"), call save_decision with the human's words. Design taste only —
never one-off content edits like typos or copy tweaks.

