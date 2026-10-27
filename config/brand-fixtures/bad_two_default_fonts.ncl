# Must be rejected: two default fonts. `font::DEFAULT` would silently be the
# first, while `font::ALL` marked both as the default.
let schema = import "../brand-schema.ncl" in
{
  schema_version = "1",
  colour = {
    tokens = [
      { name = "pitch", value = "#0c0c0e", usage = "x", group = 'ground, themes = { night = "#0c0c0e" } },
    ],
  },
  font = {
    tokens = [
      { name = "vt323", value = "VT323", usage = "x", file = "VT323-Regular.ttf", licence = "OFL-1.1", licence_file = "OFL-vt323.txt", grid_px = 16, default = true },
      { name = "silkscreen", value = "Silkscreen", usage = "x", file = "Silkscreen-Regular.ttf", licence = "OFL-1.1", licence_file = "OFL-silkscreen.txt", grid_px = 8, default = true },
    ],
  },
  size = { tokens = [{ name = "px10", value = 10, usage = "x" }] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
