-- Extract audio: the sound of a video as an AAC file (.m4a), one for each video chosen.
-- ffmpeg -nostdin -n -i <in> -vn -c:a aac -b:a 192k <out>.m4a

local VIDEOS = { "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "mts", "3gp", "ogv" }

rada.recipe {
  name = "extract-audio",
  title = { en = "Extract audio", it = "Estrai l'audio" },
  description = {
    en = "The sound of the video as an .m4a file (ffmpeg)",
    it = "Il suono del video in un file .m4a (ffmpeg)",
  },
  when = { ext = VIDEOS },
  available = function()
    return rada.has("ffmpeg")
  end,
  run = function(ctx)
    for _, input in ipairs(ctx.selection) do
      rada.run {
        program = "ffmpeg",
        args = { "-nostdin", "-n", "-i", rada.input(1), "-vn", "-c:a", "aac", "-b:a", "192k", rada.out },
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. ".m4a"),
        timeout = 3600,
      }
    end
  end,
}
