# Must be rejected: a token named `all` generates a second `ALL` constant
# beside the per-module `ALL` table, which does not compile.
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
    ],
  },
  size = { tokens = [{ name = "all", value = 10, usage = "x" }] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
