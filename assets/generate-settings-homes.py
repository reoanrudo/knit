"""Create original, illustrative OS home images for Knit settings (not screenshots).
Run with Pillow and numpy. No network or user desktop data is accessed.
"""
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent / "settings-home"
ROOT.mkdir(exist_ok=True)
S = 2

def canvas(w, h, stops):
    y = np.linspace(0, 1, h)[:, None]
    positions = np.linspace(0, 1, len(stops))
    colors = np.array(stops)
    rgb = np.stack([np.interp(y[:, 0], positions, colors[:, k]) for k in range(3)], axis=-1)
    rgb = np.broadcast_to(rgb[:, None, :], (h, w, 3)).astype(np.uint8)
    return Image.fromarray(rgb.copy())

def wave(draw, w, h, y, amplitude, phase, color):
    xs = np.linspace(0, w, 140)
    ys = y + amplitude*np.sin(xs/w*3.8 + phase) + amplitude*.3*np.cos(xs/w*8+phase)
    draw.polygon([(0,h),*zip(xs,ys),(w,h)], fill=color)

def rounded(d, box, radius, fill):
    d.rounded_rectangle(tuple(int(n) for n in box), radius=int(radius), fill=fill)

def icon(d, x, y, size, color, kind):
    rounded(d,(x,y,x+size,y+size),size*.22,color)
    white=(250,252,255,245)
    if kind % 5 == 0:
        d.ellipse((x+size*.24,y+size*.24,x+size*.76,y+size*.76),outline=white,width=max(2,int(size*.08)))
    elif kind % 5 == 1:
        d.polygon([(x+size*.35,y+size*.23),(x+size*.77,y+size*.5),(x+size*.35,y+size*.77)],fill=white)
    elif kind % 5 == 2:
        for i in range(3):
            rounded(d,(x+size*.23,y+size*(.25+i*.2),x+size*.77,y+size*(.32+i*.2)),size*.03,white)
    elif kind % 5 == 3:
        d.ellipse((x+size*.28,y+size*.22,x+size*.72,y+size*.66),fill=white)
        rounded(d,(x+size*.23,y+size*.59,x+size*.77,y+size*.78),size*.08,white)
    else:
        for i in range(2):
            for j in range(2):
                rounded(d,(x+size*(.2+i*.33),y+size*(.2+j*.33),x+size*(.45+i*.33),y+size*(.45+j*.33)),size*.04,white)

# macOS-inspired original landscape. The dock and menu bar distinguish the home.
w,h=1440,900
im=canvas(w,h,[(49,73,130),(124,156,186),(236,190,160)])
d=ImageDraw.Draw(im,'RGBA')
d.ellipse((870,100,1180,410),fill=(255,231,204,160))
wave(d,w,h,470,90,.8,(109,127,155,255))
wave(d,w,h,580,110,1.8,(75,103,139,255))
wave(d,w,h,655,150,3.1,(51,79,121,255))
wave(d,w,h,780,100,4.1,(29,56,93,255))
d.rectangle((0,0,w,26),fill=(246,249,255,80))
d.ellipse((18,8,28,18),fill=(255,255,255,220))
for x,length in [(45,45),(110,35),(165,38),(224,32)]:
    rounded(d,(x,10,x+length,15),2,(255,255,255,160))
for x in [1290,1310,1330]:d.ellipse((x,10,x+5,15),fill=(255,255,255,190))
rounded(d,(1354,8,1414,18),3,(255,255,255,150))
rounded(d,(430,810,1010,888),20,(224,234,247,100))
colors=[(62,161,238,255),(55,167,195,255),(245,245,248,255),(246,122,69,255),(242,106,142,255),(55,164,110,255),(109,113,191,255),(82,148,224,255)]
for i,c in enumerate(colors):icon(d,449+i*68,824,50,c,i)
im.resize((1280,720),Image.Resampling.LANCZOS).save(ROOT/'mac-home.png',optimize=True)

# Windows-inspired original folded bloom, no copied wallpaper.
w,h=1440,900
im=canvas(w,h,[(13,29,69),(20,53,112),(64,115,170)])
layer=Image.new('RGBA',(w,h));d=ImageDraw.Draw(layer,'RGBA')
for j in range(28):
    t=j/27
    x=380+t*170; y=105+t*130
    color=(int(35+40*t),int(110+85*t),int(206+45*t),230)
    d.ellipse((x,y,1165-t*230,790-t*75),fill=color)
for j in range(28):
    t=j/27
    color=(int(16+30*t),int(67+70*t),int(157+65*t),245)
    d.ellipse((230+t*200,270+t*180,890-t*50,845-t*190),fill=color)
im=Image.alpha_composite(im.convert('RGBA'),layer.filter(ImageFilter.GaussianBlur(1.5)))
d=ImageDraw.Draw(im,'RGBA')
d.rectangle((0,848,w,h),fill=(232,242,254,225))
for i,c in enumerate([(38,126,224,255),(238,187,65,255),(49,158,210,255),(60,138,115,255),(236,242,252,255)]):
    icon(d,610+i*46,860,30,c,i+4)
for y in [866,876,886]:rounded(d,(1320,y,1398,y+3),1,(53,77,108,150))
im.convert('RGB').resize((1280,720),Image.Resampling.LANCZOS).save(ROOT/'windows-home.png',optimize=True)

# Android-inspired original pastel tablet home, no apps or personal data.
w,h=900,1350
im=canvas(w,h,[(162,193,185),(214,218,183),(233,210,171)])
d=ImageDraw.Draw(im,'RGBA')
d.ellipse((490,250,1020,780),fill=(249,224,175,255))
wave(d,w,h,800,120,1,(137,177,160,255))
wave(d,w,h,1000,160,2.6,(70,123,118,255))
wave(d,w,h,1210,80,4,(39,83,89,255))
fontpath='/System/Library/Fonts/Helvetica.ttc'
font=ImageFont.truetype(fontpath,130)
small=ImageFont.truetype(fontpath,25)
d.text((73,135),'10:08',font=font,fill=(33,65,67,240))
d.text((78,278),'FRIDAY, OCT 16',font=small,fill=(43,78,76,200))
rounded(d,(70,400,830,474),36,(248,249,237,195))
d.ellipse((103,422,132,451),outline=(81,124,117,255),width=5)
d.line((126,446,139,458),fill=(81,124,117,255),width=5)
for row in range(2):
    for i,c in enumerate([(225,175,94,255),(89,150,143,255),(113,143,175,255),(194,128,117,255)]):
        icon(d,100+i*190,715+row*170,98,c,row*4+i)
        rounded(d,(119+i*190,827+row*170,179+i*190,833+row*170),3,(248,250,240,150))
rounded(d,(70,1150,830,1300),50,(224,239,224,100))
for i,c in enumerate([(90,171,134,255),(92,154,175,255),(205,158,94,255),(207,136,120,255)]):
    icon(d,115+i*180,1178,90,c,i)
rounded(d,(350,1321,550,1329),4,(248,250,240,230))
im.resize((600,900),Image.Resampling.LANCZOS).save(ROOT/'android-home.png',optimize=True)
print('Created 3 original illustrative home images:',ROOT)
