--- @class ByteStream
local ByteStream = {}

--- @return string?
--- @return string? error
function ByteStream:to_string() end

--- @return Value?
--- @return string? error
function ByteStream:parse_json() end

--- @param fn function
--- @param ... any
--- @return function
function bind(fn, ...) end
