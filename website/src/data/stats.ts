/** Figures measured on the product; keep them in sync with the dock's own plan. */
export const figures = {
  memory: { value: "3.1", unit: "MB", label: "dock memory at idle" },
  cpu: { value: "0", unit: "%", label: "cpu when idle" },
  installer: { value: "2.4", unit: "MB", label: "installer" },
  admin: { value: "0", unit: "", label: "admin rights needed" },
} as const;

/** The stat strip, in the order the landing shows them. */
export const stats = Object.values(figures);
