"""Generate the Ohboy ASCII art watermark PNG for Windows Terminal."""
from PIL import Image, ImageDraw, ImageFont
import os

width, height = 2750, 1100
img = Image.new("RGBA", (width, height), (0, 0, 0, 0))
draw = ImageDraw.Draw(img)

art = r"""
  ___  _     _
 / _ \| |__ | |__   ___  _   _
| | | | '_ \| '_ \ / _ \| | | |
| |_| | | | | |_) | (_) | |_| |
 \___/|_| |_|_.__/ \___/ \__, |
                         |___/
""".strip().split("\n")

try:
    font = ImageFont.truetype("C:/Windows/Fonts/consola.ttf", 130)
except Exception:
    font = ImageFont.load_default()

# Tokyo Night Storm blue at low alpha
color = (122, 162, 247, 55)

# Draw each line multiple times with small offsets to thicken strokes
y = 75
for line in art:
    for dx in range(-2, 3):
        for dy in range(-2, 3):
            draw.text((75 + dx, y + dy), line, font=font, fill=color)
    y += 150

out = os.path.join(os.environ.get("USERPROFILE", "."), "Pictures", "ohboy-watermark.png")
os.makedirs(os.path.dirname(out), exist_ok=True)
img.save(out, "PNG")
print(f"Saved: {out}")
