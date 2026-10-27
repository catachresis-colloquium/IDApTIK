# Contracts for the brand tokens (docs/design/brand/README.md).
#
# `brand-tokens.ncl` is the single source of truth for every colour, font,
# type size and spacing value the frontend draws with. This schema is what
# makes a wrong value a typecheck failure at authoring time rather than a
# surprise on screen:
#
#   - a colour is a lowercase `#rrggbb` string, and its `themes.night` entry
#     must equal its `value` (one theme today; the shape is the one the
#     claude.ai Design System artifact re-imports, see the brand book);
#   - a token name is a lowercase identifier, so it can be upper-cased into a
#     Rust constant by `crates/idaptik-brand` without any renaming rule;
#   - names are unique within a family, so the generated constants cannot
#     collide;
#   - pixel values are non-negative integers (the art is on a pixel grid).
#
# Records are closed: an unknown field is rejected, which catches typos.

let is_hex = fun v => std.is_string v && std.string.is_match "^#[0-9a-f]{6}$" v in
let is_ident = fun v => std.is_string v && std.string.is_match "^[a-z][a-z0-9_]*$" v in
let is_px = fun v => std.is_number v && std.number.is_integer v && v >= 0 in

# Whether an array of strings contains a duplicate (same helper as
# scenario-schema.ncl).
let has_duplicate = fun ids =>
  let sorted = std.array.sort (fun a b => std.string.compare a b) ids in
  let n = std.array.length sorted in
  if n < 2 then
    false
  else
    std.array.generate (fun i => std.array.at i sorted == std.array.at (i + 1) sorted) (n - 1)
    |> std.array.any (fun d => d)
in

let UniqueNames =
  std.contract.from_predicate (fun family =>
    !(has_duplicate (std.array.map (fun t => t.name) family.tokens))
  )
in

{
  Hex
    | doc "A colour as lowercase #rrggbb."
    = std.contract.from_predicate is_hex,

  Ident
    | doc "A token name: lowercase letters, digits and underscores."
    = std.contract.from_predicate is_ident,

  Px
    | doc "A whole number of pixels, never negative."
    = std.contract.from_predicate is_px,

  Group
    | doc "Which part of the palette a colour belongs to."
    = [| 'ground, 'accent, 'scene |],

  Seat
    | doc "The seat law: who a colour belongs to, if anyone."
    = [| 'hacker, 'infiltrator, 'threat, 'none |],

  ColourToken
    | doc "A named colour with its usage rule and its per-theme value."
    = std.contract.Sequence [
      {
        name | Ident,
        value | Hex,
        usage | String,
        group | Group,
        seat | Seat | default = 'none,
        themes | { night | Hex },
      },
      std.contract.from_predicate (fun t => t.value == t.themes.night),
    ],

  FontToken
    | doc "A bundled typeface: its family name, file, licence and pixel grid."
    = {
      name | Ident,
      value | String,
      usage | String,
      file | String,
      licence | String,
      licence_file | String,
      grid_px | Px,
      default | Bool | default = false,
    },

  PxToken
    | doc "A named pixel measure (type size, spacing, radius, grid)."
    = {
      name | Ident,
      value | Px,
      usage | String,
    },

  Family = fun Token => std.contract.Sequence [{ tokens | Array Token }, UniqueNames],

  Brand
    | doc "The whole token set, one family per kind."
    = {
      schema_version | String,
      colour | Family ColourToken,
      font | Family FontToken,
      size | Family PxToken,
      space | Family PxToken,
      radius | Family PxToken,
      grid | Family PxToken,
    },
}
