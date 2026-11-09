/** iPhone, iPad and iPadOS (which reports itself as a Mac with a touch screen). False without a `navigator`. */
export function isApplePhoneOrTablet(nav: Pick<Navigator, "userAgent" | "maxTouchPoints"> | undefined = typeof navigator === "undefined" ? undefined : navigator): boolean {
  if (!nav) return false;
  if (/iPhone|iPad|iPod/.test(nav.userAgent)) return true;
  return /Macintosh/.test(nav.userAgent) && (nav.maxTouchPoints ?? 0) > 1;
}
