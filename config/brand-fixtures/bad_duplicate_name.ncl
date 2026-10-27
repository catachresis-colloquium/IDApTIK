# Must be rejected: two tokens in one family with the same name would generate
# two Rust constants with the same identifier.
let schema = import "../brand-schema.ncl" in
{
  schema_version = "1",
  colour = { tokens = [] },
  font = {
    tokens = [
      { name = "vt323", value = "VT323", usage = "x", file = "VT323-Regular.ttf", licence = "OFL-1.1", licence_file = "OFL-vt323.txt", grid_px = 16, default = true },
    ],
  },
  size = {
    tokens = [
      { name = "px10", value = 10, usage = "x" },
      { name = "px10", value = 11, usage = "y" },
    ],
  },
  space = { tokens = [] },
  radius = { tokens = [] },
  grid = { tokens = [] },
} | schema.Brand
