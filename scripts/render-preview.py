"""Render exported Ratatui cells for visual QA; requires Pillow, never alters assets."""
import json
import sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

data = json.load(sys.stdin)
font_path = "/System/Library/Fonts/Menlo.ttc"
font = ImageFont.truetype(font_path, 18)
cell_width, cell_height, padding = 11, 25, 28
image = Image.new("RGB", (data["width"] * cell_width + padding * 2,
                         data["height"] * cell_height + padding * 2), "#0c1117")
draw = ImageDraw.Draw(image)
for index, cell in enumerate(data["cells"]):
    x = padding + (index % data["width"]) * cell_width
    y = padding + (index // data["width"]) * cell_height
    draw.rectangle((x, y, x + cell_width - 1, y + cell_height - 1), fill=cell["bg"])
    draw.text((x, y), cell["symbol"], fill=cell["fg"], font=font)
path = Path(sys.argv[1])
path.parent.mkdir(parents=True, exist_ok=True)
image.save(path)
