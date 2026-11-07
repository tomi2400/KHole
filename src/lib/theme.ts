/**
 * Dark by default, follows the system appearance when it says otherwise.
 * shadcn/ui switches on the `.dark` class of <html>.
 */
export function followSystemTheme(): void {
  const media = window.matchMedia("(prefers-color-scheme: light)");
  const apply = () => document.documentElement.classList.toggle("dark", !media.matches);
  apply();
  media.addEventListener("change", apply);
}
