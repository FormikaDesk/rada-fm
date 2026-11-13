-- Resize image: a smaller copy whose longer side is at most the size chosen, one for each image.
-- magick <in> -resize 1600x1600> <out>; on a Mac without ImageMagick, sips -Z 1600 <in> --out <out>.
-- The ">" is ImageMagick's own: only images bigger than that are made smaller. No shell ever
-- sees it.

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
  name = "resize-image",
  title = { en = "Resize image", it = "Ridimensiona immagine" },
  description = {
    en = "A smaller copy, the longer side at most 1600, 1080 or 800 pixels",
    it = "Una copia più piccola, il lato lungo al massimo di 1600, 1080 o 800 pixel",
  },
  when = { ext = IMAGES },
  args = {
    { name = "size", label = { en = "Longer side", it = "Lato lungo" }, choices = { "1600", "1080", "800" }, default = "1600" },
  },
  available = function(ctx)
    return tool(ctx) ~= nil
  end,
  run = function(ctx, args)
    local program = tool(ctx)
    for _, input in ipairs(ctx.selection) do
      local call
      if program == "magick" then
        call = { rada.input(1), "-resize", args.size .. "x" .. args.size .. ">", rada.out }
      else
        call = { "-Z", args.size, rada.input(1), "--out", rada.out }
      end
      local ext = string.lower(rada.ext(input) or "png")
      rada.run {
        program = program,
        args = call,
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. "-" .. args.size .. "." .. ext),
        size_hint = size(input),
      }
    end
  end,
}
