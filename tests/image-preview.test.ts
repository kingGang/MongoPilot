import { describe, it, expect } from "vitest";
import { imageUrlCandidate } from "../src/utils/image-preview";

describe("imageUrlCandidate", () => {
  it("接受常见图片后缀的 http/https 链接", () => {
    for (const u of [
      "https://cdn.example.com/a/b.png",
      "http://example.com/x.JPG",
      "https://example.com/i.webp?v=2",
      "https://example.com/i.gif#frag",
    ]) {
      expect(imageUrlCandidate(u), u).toBe(u);
    }
  });

  it("接受 data:image/ 内联图片", () => {
    const u = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
    expect(imageUrlCandidate(u)).toBe(u);
  });

  it("没有后缀的对象存储/CDN 链接也放行 (交给实际加载判定)", () => {
    const u = "https://hotrefresh.myversion.cn/item/8f21c0d3a94b";
    expect(imageUrlCandidate(u)).toBe(u);
  });

  it("明显不是图片的后缀直接排除, 不发请求", () => {
    for (const u of [
      "https://api.example.com/v1/items.json",
      "https://example.com/doc.pdf",
      "https://example.com/pkg.zip",
      "https://example.com/clip.mp4",
      "https://example.com/page.html",
    ]) {
      expect(imageUrlCandidate(u), u).toBeNull();
    }
  });

  it("非字符串 / 非 http 链接 / 过短过长都不认", () => {
    expect(imageUrlCandidate(123)).toBeNull();
    expect(imageUrlCandidate(null)).toBeNull();
    expect(imageUrlCandidate({ $oid: "x" })).toBeNull();
    expect(imageUrlCandidate("enabled")).toBeNull();
    expect(imageUrlCandidate("ftp://example.com/a.png")).toBeNull();
    expect(imageUrlCandidate("/local/path/a.png")).toBeNull();
    expect(imageUrlCandidate("https://e.com/" + "a".repeat(5000) + ".png")).toBeNull();
  });

  it("首尾空白会被 trim", () => {
    expect(imageUrlCandidate("  https://example.com/a.png  ")).toBe("https://example.com/a.png");
  });
});
