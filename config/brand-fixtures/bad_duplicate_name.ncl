# Must be rejected: two tokens in one family with the same name would generate
# two Rust constants with the same identifier.
let schema = import "../brand-schema.ncl" in
{
  schema_version = "1",
  colour = { tokens = [] },
  font = { tokens = [] },
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
