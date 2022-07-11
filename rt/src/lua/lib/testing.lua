local testing = {}

testing.assert = assert

--- Throws error if `value` is a truthy value (all values except `false` and `nil`).
---
--- @param value any
--- @param msg any?
function testing.assert_false(value, msg)
  assert(not value, msg)
end

--- Throws error if `left` or `right` are not equal.
---
--- @generic T
--- @param left T
--- @param right T
--- @param msg any?
function testing.assert_eq(left, right, msg)
  if left ~= right then
    local msg = msg or ("assertion failed: left == right\n" ..
      "\tleft = " .. debug_fmt(left) .. "\n" ..
      "\tright = " .. debug_fmt(right) .. "\n")
    error(msg)
  end
end

--- Throws error if `left` or `right` are equal.
---
--- @generic T
--- @param left T
--- @param right T
--- @param msg any?
function testing.assert_ne(left, right, msg)
  if left == right then
    local msg = msg or ("assertion failed: left ~= right\n" ..
      "\tleft = " .. debug_fmt(left) .. "\n" ..
      "\tright = " .. debug_fmt(right) .. "\n")
    error(msg)
  end
end

return testing
