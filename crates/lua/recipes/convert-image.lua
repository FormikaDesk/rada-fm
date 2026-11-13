-- Convert image: a copy in another format, one for each image chosen.
-- magick <in> <out>; on a Mac without ImageMagick, sips -s format <format> <in> --out <out>.
-- (On Linux "magick" may be ImageMagick's older "convert"; on Windows never: there "convert"
-- is another program. The table of programs decides, not this file.)

local IMAGES = { "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif", "avif" }

local function tool(ctx)
  if rada.has("magick") then
    return "magick"
  end
  if ctx.platform == "macos" and rada.has("sips") then
    return "sips"
  end
  return nil
end

local function size(path)
  local s = rada.stat(path)
  return s and s.size or nil
end

rada.recipe {
  name = "convert-image",
  title = { en = "Convert image", it = "Converti immagine" },
  description = {
    en = "A copy in PNG, JPEG or WebP (ImageMagick, or sips on a Mac)",
    it = "Una copia in PNG, JPEG o WebP (ImageMagick, o sips su Mac)",
  },
  when = { ext = IMAGES },
  args = {
    { name = "format", label = { en = "Format", it = "Formato" }, choices = { "png", "jpg", "webp" }, default = "png" },
  },
  available = function(ctx)
    return tool(ctx) ~= nil
  end,
  run = function(ctx, args)
    local program = tool(ctx)
    if program == "sips" and args.format == "webp" then
      if ctx.lang == "it" then
        error("sips non scrive WebP: serve ImageMagick", 0)
      end
      error("sips does not write WebP: it takes ImageMagick", 0)
    end
    for _, input in ipairs(ctx.selection) do
      local call
      if program == "magick" then
        call = { rada.input(1), rada.out }
      else
        local format = args.format == "jpg" and "jpeg" or args.format
        call = { "-s", "format", format, rada.input(1), "--out", rada.out }
      end
      -- A PNG made into a PNG is a copy beside it, never a name that is the input's own.
      local ext = string.lower(rada.ext(input) or "")
      local same = ext == args.format or (args.format == "jpg" and ext == "jpeg")
      local stem = same and (rada.stem(input) .. "-converted") or rada.stem(input)
      rada.run {
        program = program,
        args = call,
        inputs = input,
        output = rada.join(rada.parent(input), stem .. "." .. args.format),
        size_hint = size(input),
      }
    end
  end,
}
