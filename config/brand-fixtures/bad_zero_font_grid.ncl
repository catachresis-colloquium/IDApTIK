# Must be rejected: a zero font grid. `fonts::UiFont::snap` divides by the grid,
# so every size in that face would snap to 0 px and the text would vanish.
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
      { name = "vt323", value = "VT323", usage = "x", file = "VT323-Regular.ttf", licence = "OFL-1.1", licence_file = "OFL-vt323.txt", grid_px = 0, default = true },
    ],
  },
  size = { tokens = [{ name = "px10", value = 10, usage = "x" }] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
