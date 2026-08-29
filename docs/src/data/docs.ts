export const siteConfig = {
  name: "cargo-warm",
  strapline: "Warm Rust worktrees without shared mutable state",
  description:
    "Fork warm Cargo build state into isolated worktrees so new Rust checkouts can start much closer to warm.",
  repoUrl: "https://github.com/amxv/cargo-warm",
  accentColor: "#b7410e",
  accentColorDark: "#f28c52",
  footerSections: [
    {
      title: "cargo-warm",
      text: "Private writable Cargo state per worktree, seeded cheaply from an already-warm checkout."
    },
    {
      title: "Correctness",
      text: "Cargo and rustc remain responsible for freshness and incremental validation after every seed."
    },
    {
      title: "Repository",
      linkPrefix: "Source: ",
      linkHref: "https://github.com/amxv/cargo-warm",
      linkLabel: "github.com/amxv/cargo-warm"
    }
  ]
} as const;

export const docCategories = ["Start", "Concepts", "Operations", "Reference"] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
