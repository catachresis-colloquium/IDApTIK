# Must be rejected: a colour whose theme entry drifted from its value.
let schema = import "../brand-schema.ncl" in
{
  schema_version = "1",
  colour = {
    tokens = [
      { name = "pitch", value = "#0c0c0e", usage = "x", group = 'ground, themes = { night = "#141416" } },
    ],
  },
  font = { tokens = [] },
  size = { tokens = [] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
