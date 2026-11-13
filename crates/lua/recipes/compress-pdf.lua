-- Compress PDF: a lighter copy made by Ghostscript, one for each PDF chosen.
-- gs -sDEVICE=pdfwrite -dPDFSETTINGS=/<quality> -dNOPAUSE -dBATCH -dSAFER -sOutputFile=<out> <in>

local function size(path)
  local s = rada.stat(path)
  return s and s.size or nil
end

rada.recipe {
  name = "compress-pdf",
  title = { en = "Compress PDF", it = "Comprimi PDF" },
  description = {
    en = "A lighter copy: screen is the smallest, printer the best (Ghostscript)",
    it = "Una copia più leggera: screen è la più piccola, printer la migliore (Ghostscript)",
  },
  when = { ext = "pdf" },
  args = {
    { name = "quality", label = { en = "Quality", it = "Qualità" }, choices = { "screen", "ebook", "printer" }, default = "ebook" },
  },
  available = function()
    return rada.has("gs")
  end,
  run = function(ctx, args)
    for _, input in ipairs(ctx.selection) do
      rada.run {
        program = "gs",
        args = {
          "-sDEVICE=pdfwrite",
          "-dPDFSETTINGS=/" .. args.quality,
          "-dNOPAUSE",
          "-dBATCH",
          "-dSAFER",
          rada.out_after("-sOutputFile="),
          rada.input(1),
        },
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. "-" .. args.quality .. ".pdf"),
        size_hint = size(input),
        timeout = 1800,
      }
    end
  end,
}
