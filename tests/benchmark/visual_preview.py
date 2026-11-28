"""Test the helper with locally generated PNG and two-page vector PDF."""
import struct, subprocess, sys, zlib
def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag+data))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 2, 0, 0, 0)) + chunk(b"IDAT",zlib.compress(b"\x00\xff\x00\x00\x00\xff\x00"*2)) + chunk(b"IEND",b"")
objs = [b"<< /Type /Catalog /Pages 2 0 R >>",b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 5 0 R >>",
b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 6 0 R >>"]
for color in [b"1 0 0", b"0 0 1"]:
    stream=color+b" rg 0 0 100 100 re f\n"
    objs.append(b"<< /Length "+str(len(stream)).encode()+b" >>\nstream\n"+stream+b"endstream")
pdf=b"%PDF-1.4\n"; offsets=[0]
for i,obj in enumerate(objs,1):
    offsets.append(len(pdf)); pdf+=str(i).encode()+b" 0 obj\n"+obj+b"\nendobj\n"
xref=len(pdf)
pdf+=b"xref\n0 7\n0000000000 65535 f \n"+b"".join(f"{o:010d} 00000 n \n".encode() for o in offsets[1:])+b"trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n"+str(xref).encode()+b"\n%%EOF\n"
def main():
    for kind, page, data, count in [("image","1",png,1),("pdf","1",pdf,2),("pdf","2",pdf,2)]:
        result=subprocess.run([sys.argv[1],"rgb-v1",kind,page,"40","40"],input=data,capture_output=True,check=True,timeout=10).stdout
        assert result[:8]==b"TTVRGB01"
        w,h,pages=struct.unpack("<III",result[8:20])
        assert 0<w<=40 and 0<h<=40 and pages==count and len(result)==20+w*h*3
        if kind=="pdf":
            center=result[20+((h//2)*w+w//2)*3:][:3]
            assert center == (b"\xff\x00\x00" if page=="1" else b"\x00\x00\xff"),center
    assert subprocess.run([sys.argv[1],"rgb-v1","image","1","40","40"],input=b"invalid",capture_output=True,timeout=10).returncode!=0
    print("Visual preview PNG and two-page PDF fixtures passed")

if __name__=="__main__":
    main()
