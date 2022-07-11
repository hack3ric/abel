--- @class json
local json = {}

--- Any valid JSON value.
---
--- Note that recursion in tables is not allowed.
---
--- @alias Value nil | boolean | number | string | table<string | integer, Value>

--- @param str string
--- @return Value?
--- @return string? error
function json.parse(str) end

--- @param value Value
--- @param pretty? boolean
--- @return string?
--- @return string? error
function json.stringify(value, pretty) end

--- @param table table
--- @return table
function json.array(table) end

--- @param table table
--- @return table
function json.undo_array(table) end

--- @type table
json.array_metadata = {}

return json
