-- What currently works

--- @class Stream__string
local Stream__string = {}

--- @return string
function Stream__string:read() end

--- @class BufStream: Stream__string
local BufStream = {}

--- @param mode ReadMode?
--- @return string
function BufStream:read(mode) end

--- @class Sink__string
--- @field write fun(self: Sink__string, item: string)

--- @class Transform__string_string
--- @field transform fun(self: Transform__string_string, item: string): string

-- What I hope it works

-- - @class Stream<T>
-- - @field read fun(self: Stream<T>): T

-- - @class Sink<T>
-- - @field write fun(self: Sink<T>, item: T)

-- - @class Transform<T, U>
-- - @field transform fun(self: Transform<T, U>, item: T): U

