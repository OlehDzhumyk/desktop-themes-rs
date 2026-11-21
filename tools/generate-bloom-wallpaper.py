# SPDX-License-Identifier: GPL-3.0-or-later
import sys, math
import numpy as np
from PIL import Image, ImageFilter
W,H=3840,2160
dark = sys.argv[1]=='dark'
out=sys.argv[2]
# background
yy,xx=np.mgrid[0:H,0:W].astype(np.float32)
cx,cy=W*0.5,H*0.5
r=np.hypot((xx-cx)/W,(yy-cy)/H*1.0)
if dark:
    c0=np.array([8,14,40]);c1=np.array([2,4,14])
else:
    c0=np.array([226,238,252]);c1=np.array([170,200,240])
t=np.clip(r/0.8,0,1)[...,None]
img=(c0*(1-t)+c1*t)
# petals: rotated ellipses with gradient, drawn on layers
def petal(angle,color,alpha,length,width):
    ca,sa=math.cos(angle),math.sin(angle)
    dx=xx-cx;dy=yy-cy
    u=dx*ca+dy*sa   # along petal
    v=-dx*sa+dy*ca
    # petal centered at u=length/2
    uu=(u-length*0.5)/(length*0.5)
    vv=v/(width*0.5*(1-0.55*np.clip(uu,-1,1)**2*0+0))
    d=uu**2+vv**2
    m=np.clip(1-d,0,1)**0.8*alpha
    return m[...,None]*np.array(color)
base_petals=[]
n=7
cols_l=[(30,110,240),(90,70,230),(20,170,250),(120,90,240),(40,130,255),(70,60,200),(10,150,240)]
cols_d=[(30,100,255),(110,60,255),(10,160,255),(150,70,255),(40,120,255),(90,50,220),(20,140,250)]
cols=cols_d if dark else cols_l
canvas=img.copy()
for i in range(n):
    a=i*2*math.pi/n+0.4
    m=petal(a,cols[i],1.0,H*0.78,H*0.42)
    al=np.clip(np.linalg.norm(m,axis=2)/np.linalg.norm(cols[i]),0,1)[...,None]*(0.55 if not dark else 0.65)
    canvas=canvas*(1-al)+np.array(cols[i])*al
# bright core glow
g=np.exp(-((xx-cx)**2+(yy-cy)**2)/(2*(H*0.10)**2))[...,None]
canvas=canvas*(1-g*0.8)+np.array([235,245,255] if not dark else [140,170,255])*g*0.8
im=Image.fromarray(np.clip(canvas,0,255).astype('uint8')).filter(ImageFilter.GaussianBlur(14))
im.save(out,optimize=True)
