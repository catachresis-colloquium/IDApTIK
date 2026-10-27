# Must be ACCEPTED: the smallest valid token set, and the base every bad_*.ncl
# fixture departs from by exactly one defect. `just brand-check` requires this
# to export, so a bad fixture cannot pass for the wrong reason (a base that the
# schema rejects anyway would make every bad fixture "rejected as expected").
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
  size = { tokens = [{ name = "px10", value = 10, usage = "x" }] },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
