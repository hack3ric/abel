--- @class crypto
local crypto = {}

--- @type Rng
crypto.thread_rng = {}

--- @class Rng
local Rng = {}

--- @return number
function Rng:random() end

--- @param low integer
--- @param high integer
--- @return integer
function Rng:gen_range(low, high) end

return crypto
