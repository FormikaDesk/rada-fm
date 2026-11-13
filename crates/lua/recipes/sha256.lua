-- SHA-256: rada computes the checksum itself and writes <name>.sha256 next to each file, in the
-- format of sha256sum. No program runs.

rada.recipe {
  name = "sha256",
  title = { en = "SHA-256 checksum", it = "Checksum SHA-256" },
  description = {
    en = "Writes name.sha256 next to the file, as sha256sum does",
    it = "Scrive nome.sha256 accanto al file, come sha256sum",
  },
  when = { kind = "file" },
  run = function(ctx)
    for _, input in ipairs(ctx.selection) do
      rada.checksum { input = input, output = input .. ".sha256" }
    end
  end,
}
