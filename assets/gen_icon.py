# Генерация assets/icon.ico для DarkTechMD (одноразовый скрипт).
# Стиль приложения: плоско, тёмный фон #1B1B1E, «M» шрифтом JetBrains Mono
# (вариативный, wght 700), акцентное подчёркивание #6EA8FE — как у заголовков
# h1/h2 в читалке.
from PIL import Image, ImageDraw, ImageFont

BG = (27, 27, 30, 255)       # pal.bg тёмной темы
FG = (214, 214, 210, 255)    # pal.fg
ACCENT = (110, 168, 254, 255)  # pal.link

FONT = "../assets/fonts/JetBrainsMono-VF.ttf"

S = 256
img = Image.new("RGBA", (S, S), BG)
d = ImageDraw.Draw(img)

font = ImageFont.truetype(FONT, 150)
try:
    font.set_variation_by_axes([700])  # wght 700, как заголовки
except Exception as e:
    print("variation:", e)

# Буква M по центру, чуть выше середины (ниже — полоса).
bbox = d.textbbox((0, 0), "M", font=font)
w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
x = (S - w) / 2 - bbox[0]
y = (S - h) / 2 - bbox[1] - 14
d.text((x, y), "M", font=font, fill=FG)

# Акцентная полоса-подчёркивание (стиль h1/h2 читалки).
bar_h = 18
bar_w = int(S * 0.62)
bx = (S - bar_w) / 2
by = y + bbox[3] + 16
d.rectangle([bx, by, bx + bar_w, by + bar_h], fill=ACCENT)

img.save("../assets/icon.ico", sizes=[
    (16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256),
])
img.save("icon_preview.png")
print("ok")
