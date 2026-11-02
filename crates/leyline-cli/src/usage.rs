//! The `leyline help` text: every command, its arguments, and the ADR behind it.

pub(crate) const USAGE: &str = "\
Leyline — open-source RAW photo development

Usage:
  leyline new <library> [--name <name>]
  leyline info <library>
  leyline roots <library>           the roots it references, and which are online
  leyline root-add <library> <folder> [--name <name>]
                                    lets the library reference photos inside
                                    that folder, wherever the folder later moves
  leyline root-locate <library> <uuid> <folder>
                                    says where a root went; refused unless the
                                    folder's marker agrees
  leyline root-forget <library> <uuid>
                                    refused while it still holds photographs
  leyline import <library> <source> [--reference] [--flat] [--no-pair]
                                    [--thumbnails] [--only <name>]...
                                    --only, repeatable, imports only those
                                    files among the ones `scan` lists
  leyline scan <library> <source> [--flat] [--exact]
                                    what an import would take, writing nothing
                                    (ADR 0065); « = » marks a file the library
                                    already holds — by name+size, or exactly with
                                    --exact, which reads each file in full and
                                    names the asset (ADR 0095)
  leyline auto-tone <library> <version-id> [--dry-run]
                                    proposes a tone and writes it (ADR 0088);
                                    --dry-run prints without committing
  leyline auto-tca <library> <version-id> [--dry-run]
                                    measures the lens's chromatic aberration on
                                    this photo and writes it to the revision
                                    (ADR 0111); --dry-run only prints it
  leyline keystone <library> <version-id> --line x1,y1,x2,y2 [--line …] [--dry-run]
                                    solves the two perspective sliders from lines
                                    you drew along what ought to be straight
                                    (ADR 0119); nothing is detected — each line's
                                    slope says whether it should be vertical or
                                    horizontal; --dry-run writes nothing
  leyline rename <library> <template> <asset-id>...
                                    renames the files on disk (ADR 0100):
                                    {name} {date} {time} {seq}, the extension is
                                    kept; a name already taken is refused, never
                                    overwritten, and RAW+JPEG companions follow
  leyline detectors [--from <dir>]  the mask detectors installed for this user
                                    (ADR 0073); none ships with Leyline. --from
                                    reads manifests elsewhere: what a detector
                                    author uses before installing
  leyline detect <library> <version-id> <detector:detection>
                                    runs the detection on the preview and adds a
                                    local adjustment carrying the coverage it
                                    returned (ADR 0105)
  leyline detect-check <detector:detection>
                                    checks a detector speaks the protocol, on a
                                    synthetic image (ADR 0105 §3); never judges
                                    the quality of a cut-out
  leyline cull <library> [asset-id...] [--burst-distance <bits>] [--apply]
                                    walks the photos and PROPOSES which to
                                    reject (ADR 0084): bursts grouped, the
                                    sharpest kept, black or blown frames named.
                                    Writes nothing without --apply
  leyline processors [--from <dir>] the pixel processors installed for this
                                    user (ADR 0107); none ships with Leyline.
                                    --from reads manifests elsewhere
  leyline derive <library> <version-id> <processor:operation>
                                    develops the photo up to rank 20, hands it to
                                    the processor, and files the answer as a NEW
                                    ASSET carrying the original's development
                                    (ADR 0107)
  leyline derive-check <processor:operation>
                                    checks a processor speaks the protocol, on a
                                    synthetic image (ADR 0107 §8); never judges
                                    the quality of a denoise
  leyline describe <library> <asset-id> [--title <t>] [--caption <c>]
               [--creator <n>] [--copyright <c>] [--credit <c>]
               [--city <c>] [--state <s>] [--country <c>] [--clear]
                                    what one writes about a photo (ADR 0099):
                                    kept whatever happens to the file, and
                                    carried into the XMP sidecar; with no option,
                                    prints what is written
  leyline versions <library> <version-id>
                                    the developments of this photo (ADR 0094),
                                    the current one starred
  leyline version-create <library> <version-id> [name]
                                    branches a second development and makes it
                                    current; the name is generated if absent
  leyline version-switch <library> <version-id>
                                    makes this development the current one —
                                    the graph is never rewritten
  leyline sample-range <library> <version-id> <x,y>
                                    luminance (display axis) and hue under the
                                    [0,1]² point (ADR 0093) — to write a
                                    local-adjustment payload by hand
  leyline auto-wb <library> <version-id> [--sample x,y] [--dry-run]
                                    measured white balance (ADR 0091):
                                    grey-world without --sample, eyedropper on
                                    the [0,1]² point with it; --dry-run writes
                                    nothing
  leyline tether <library> [--session <name>] [--preset <name>]
               [--set <setting>=<value>] [--capture-every <seconds>]
                                    tethered capture (ADR 0038, ADR 0087):
                                    --session is the folder under Photos/,
                                    --preset develops each photo as it arrives,
                                    --set drives the body (shutter, aperture,
                                    iso, wb) and --capture-every releases the
                                    shutter at an interval
  leyline watch <library> <folder>
                                    imports each file that settles in the
                                    folder, until Ctrl+C (ADR 0039)
  leyline ls <library> [--text <query>] [--rating <min>]
               [--camera <name>] [--lens <name>] [--iso <range>]
               [--aperture <range>] [--focal <range>] [--shutter <range>]
               [--collection <id>]
                                    shot filters (ADR 0064); a range is written
                                    <min>-<max>, <min>- or -<max>, and speeds
                                    may be fractions: --shutter -1/500;
                                    --collection lists its members, a manual
                                    one in its own order (ADR 0169)
  leyline collections <library>     the collection tree: id, manual or smart,
                                    member count; a smart one prints its rules
  leyline collection-create <library> <name> [--parent <id>]
               [--rating <1-5>] [--camera <name>] [--keyword <path>]... [--pick]
                                    manual, or smart as soon as one rule is
                                    given; a keyword is a path (Nature/Oiseaux)
                                    and one the library lacks is refused
  leyline collection-add <library> <collection-id> <version-id>...
  leyline collection-remove <library> <collection-id> <version-id>...
                                    a smart collection is filled by its rules,
                                    never by hand
  leyline collection-rename <library> <collection-id> <name>
  leyline collection-move <library> <collection-id> (--parent <id> | --root)
  leyline collection-delete <library> <collection-id>
                                    with everything under it; no version and
                                    no file is touched
  leyline facets <library>          the bodies, lenses and value ranges the
                                    library actually holds — what the filters
                                    above take
  leyline mcp --library <library>   serve the library to an AI agent over
                                    stdio (Model Context Protocol, ADR 0154)
  leyline preview <library> <asset-id> [--kind <thumbnail|small|medium|large|full>]
  leyline verify <library> <file>   say whether the file is one this library
                                    exported: intact, modified since, or
                                    unknown here (ADR 0153). Exits 0 only
                                    when intact
  leyline reproduce <library> <file> <dest-dir>
                                    render the export the file came from
                                    again into <dest-dir> and compare: bit
                                    for bit, same pixels (metadata moved),
                                    or different pixels (ADR 0153). Exits 0
                                    only when bit for bit
  leyline export <library> <dest-dir> <version-id>...
                 [--preset <name>] [--format <f>] [--quality <1-100>] [--max-edge <px>]
                 [--avif-speed <1-10>] [--concurrency <n>] [--metadata <m>]
                 [--watermark <text>] [--watermark-anchor <a>]
                 [--watermark-size <%>] [--watermark-color <#RRGGBB>]
                 [--watermark-opacity <0-1>]
  leyline preset <library> <name> [--format <f>] [--quality <1-100>] [--max-edge <px>]
                 [--avif-speed <1-10>] [--concurrency <n>] [--metadata <m>]
                 [--watermark <text>] [--watermark-anchor <a>]
                 [--watermark-size <%>] [--watermark-color <#RRGGBB>]
                 [--watermark-opacity <0-1>]
  leyline presets <library>
  leyline exports <library> <asset-id>
  leyline print <library> <dest-dir> <version-id>...
                [--preset <name>] [--paper <a4|a3|letter|<w>x<h>mm>] [--orientation <portrait|landscape>]
                [--margins <mm>] [--dpi <n>] [--profile <path>] [--intent <intent>] [--copies <n>]
  leyline print-preset <library> <name> [print options above, minus --preset]
  leyline print-presets <library>
  leyline contact-sheet <library> <sheet.pdf> <version-id>...
                [--preset <name>] [print page options above, minus --copies]
                [--columns <n>] [--rows <n>] [--gutter <mm>]
                [--captions <filename|none>] [--caption-size <mm>]
  leyline contact-sheet-preset <library> <name> [contact-sheet options above, minus --preset]
  leyline contact-sheet-presets <library>
  leyline camera-profile <library> <file.dcp>
  leyline camera-profiles <library>
  leyline lut <library> <file.cube>
  leyline luts <library>
  leyline pair <library>            attaches each in-camera JPEG to the RAW of
                                    the same shot (ADR 0079); import already does
                                    it, this catches up what was imported before
  leyline unpair <library> <asset-id>...
                                    detaches, and the photo returns to the grid
  leyline remove <library> <asset-id>...
                                    takes the photos out of the catalog; the
                                    files are left exactly where they are
  leyline delete <library> <asset-id>... --yes
                                    same, and sends the files (and their .xmp
                                    sidecars) to the system trash
  leyline rate <library> <stars|none> <version-id>...
  leyline pick <library> <pick|reject|none> <version-id>...
  leyline label <library> <red|yellow|green|blue|purple|none> <version-id>...
  leyline develop <library> <version-id> <param> <value...>
  leyline reprocess <library> <version-id>...
  leyline history <library> <version-id>
  leyline preset-save <library> <name> <version-id> --groups <g,g,...>
                                    groups: white_balance, tone, presence,
                                    tone_curve, color_mixer, color_grading,
                                    camera_profile, creative_lut, effects,
                                    lens_correction, detail, rendering,
                                    geometry, reshape, spot_removal, red_eye,
                                    local_adjustments (ADR 0132 §1),
                                    comma-separated, no spaces; only the
                                    groups named are captured
                                    (docs/presets.md §3.1)
  leyline preset-list <library>
  leyline preset-apply <library> <name> <version-id>...
  leyline preset-rm <library> <name>
  leyline preset-update <library> <name> <version-id> --groups <g,g,...>
                                    redefines the preset from that photo and
                                    bumps its version (ADR 0058)
  leyline preset-reapply <library> <name>
                                    re-applies it to every photo still carrying
                                    an older version of it

Options:
  --reference   Reference files in place instead of copying into Photos/
  --flat        Do not descend into subdirectories
  --thumbnails  Warm the thumbnail cache after importing, and wait for it.
                Off by default: the grid fills in as it is browsed
  --format <f>  Export format: jpeg (default), png, tiff, webp, avif
  --avif-speed <1-10>
                AVIF encoder effort, 9 by default (ADR 0067): low is slow and
                thorough, 10 encodes ~6x faster for 0 to 14% more bytes. Trades
                time against size only — the image is the same. Other formats
                ignore it
  --metadata <full|without-location|none>
                What the exported file says about the photograph (ADR 0157):
                capture, camera, author, keywords — and, with full only, the
                GPS position, city, state and country. without-location by
                default; none writes only the colour profile. AVIF files
                carry none of it
  --concurrency <n>
                Photos an export batch keeps in flight, 4 by default
                (ADR 0068). Higher is faster up to about 6 and costs ~0.7 GB
                of peak memory per photo at 30 Mpx
  --intent <i>  Print rendering intent: perceptual, relative (default), saturation, absolute
  --watermark <text>
                Text watermark drawn on the export, last thing before encoding (ADR 0034)
  --watermark-anchor <a>
                bottom-right (default), bottom-left, top-right, top-left, center
  --watermark-size <%>
                Cap height as a percentage of the image height, in (0, 50] (default 3)
  --watermark-color <#RRGGBB>
                Text colour (default #FFFFFF)
  --watermark-opacity <0-1>
                Blend strength (default 0.7)

Develop params (docs/pipeline.md §3.2, schema 1):
  exposure rotation                 decimal
  contrast highlights shadows whites blacks vibrance saturation
  clarity texture dehaze           integer in [-100, 100]
  highlight-rolloff <0-100>         highlight shoulder at the output (0 = clip at white)
  highlight-reconstruction <clip|blend|rebuild>
                                    what the decoder does with channels that saturated at
                                    the sensor (ADR 0050); clip is the neutral default.
                                    Needs a revision pinned at input version 2 — reprocess
                                    an older one first
  demosaic <ahd|vng|dcb|dht>        which interpolation reconstructs the missing
                                    channels (ADR 0061); ahd is the neutral default.
                                    Needs a revision pinned at input version 3 —
                                    reprocess an older one first. Has no effect on
                                    thumbnail/small previews, which decode at half
                                    size and skip interpolation entirely
  white-balance <kelvin> <tint>     tint integer, or `white-balance none` for as-shot
  lut <path|on|off|none>            path is library-relative, as listed by `leyline luts`;
                                    on/off toggles the LUT already referenced, none removes it
  lut-strength <0-100>              how much of the look to apply (ADR 0053)
  lens-correction <on|off>
  vignette <amount> [midpoint] [roundness] [feather]
                                    the vignette you *add*, drawn on the cropped frame
                                    (ADR 0090): amount/roundness in [-100, 100] (negative
                                    amount darkens the corners), midpoint/feather in
                                    [0, 100]; the three shape values default to 50/0/50
  grain <amount> [size] [roughness] [color]
                                    film grain, each in [0, 100] (ADR 0090); size and
                                    roughness default to 25 and 50, color to 0. Color is
                                    how much the three layers disagree, and it adds
                                    chroma without touching the grain's grey (ADR 0118)
  defringe <purple> [green]         takes the saturation out of the coloured halo
                                    beside a high-contrast edge, each in [0, 100]
                                    (ADR 0113); green defaults to 0
  noise-reduction <luminance> <color>
  moire <0-100>                     colour moiré: false colour bands over fine
                                    patterns; 0 = off (ADR 0165)
  sharpening <amount> <radius> [masking]
                                    masking confines the sharpening to edges
                                    (ADR 0096); absent = 0, as before
  crop <x> <y> <width> <height>     percent 0-100, or `crop reset`
  perspective <vertical> <horizontal>
                                    keystone correction, integers in [-100, 100]
                                    (ADR 0052), or `perspective reset`
  parametric-curve <shadows> <darks> <lights> <highlights>
                   [<shadow-split> <midtone-split> <highlight-split>]
                                    four regions in -100..100 and the three
                                    splits in 1..99 (ADR 0137); `-` keeps a
                                    field, and trailing fields may be omitted
  levels <in-black> <in-white> <gamma> [<out-black> <out-white>] | reset
                                    black and white points in 0..255, gamma
                                    in 0.10..9.99 (above 1 brightens), output
                                    limits default to 0 255 (ADR 0164)
  tone-curve [channel] <x,y> <x,y>...
                                    points in [0,1], strictly increasing x, or
                                    `tone-curve reset`; channel is one of
                                    master/red/green/blue (ADR 0098), absent =
                                    the master curve
  reshape <from-x> <from-y> <to-x> <to-y> <radius> [strength]
                                    moves the content grabbed at (from) to
                                    (to), positions and radius percent 0-100,
                                    strength in [0, 1] (default 1) — ADR 0109;
                                    `reshape reset` empties the list
  red-eye <x> <y> <radius> <feather> <darken>
                                    position/radius percent 0-100,
                                    feather/darken in [0, 1]; appends one
                                    correction (ADR 0103), or `red-eye reset`
  spot-removal <tx> <ty> <sx> <sy> <radius> <feather> <opacity>
                                    positions/radius percent 0-100, feather/opacity 0-1;
                                    appends one spot, or `spot-removal reset` to clear all
  local-adjustment <json|@file>     appends one masked local adjustment (ADR 0029, ADR 0048),
                                    written exactly as `settings_json` stores it
                                    (docs/pipeline.md §3.2), e.g.
                                    '{\"mask\":{\"type\":\"radial\",\"cx\":0.5,\"cy\":0.5,
                                    \"rx\":0.3,\"ry\":0.3,\"angle\":0,\"feather\":0.5,
                                    \"inverted\":false},\"opacity\":1,
                                    \"adjustments\":{\"exposure\":-0.5}}';
                                    adjustments also accepts clarity, texture,
                                    sharpness (each in [-100, 100]) and
                                    noise_luminance, noise_color (in [0, 100]),
                                    the five neighbourhood operators of ADR 0108,
                                    plus defringe_purple and defringe_green (in
                                    [0, 100], ADR 0116);
                                    @file reads the payload from a file instead
  local-adjustment rm <index>       removes the adjustment at that index
  local-adjustment reset            removes every local adjustment
  hsl-band <band> <hue> <saturation> <luminance>
                                    band is one of red/orange/yellow/green/aqua/blue/purple/magenta,
                                    each value integer in [-100, 100]
  color-grading <shadows|midtones|highlights> <hue> <saturation> <luminance>
                                    hue integer in [0, 360), saturation in [0, 100], luminance in [-100, 100]
  color-grading balance <n>         integer in [-100, 100]
  color-grading blending <n>        integer in [0, 100]
  color-grading reset               clears every zone and balance/blending
  camera-profile <path|on|off|none>
                                    path is library-relative, as listed by
                                    `leyline camera-profiles`; on/off toggles the
                                    profile already referenced, none removes it
                                    EXPERIMENTAL: DCP colors are not yet validated
                                    against reference renders

  leyline --version                 this build, and the RAW decoder it links
";
