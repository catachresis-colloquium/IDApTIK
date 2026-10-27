# Must be rejected: a colour value that is not lowercase #rrggbb.
let schema = import "../brand-schema.ncl" in
{
  schema_version = "1",
  colour = {
    tokens = [
      { name = "pitch", value = "#0C0C0E", usage = "x", group = 'ground, themes = { night = "#0C0C0E" } },
    ],
  },
  font = { tokens = [] },
  size = { tokens = [] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
