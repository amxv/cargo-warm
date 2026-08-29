export const siteConfig = {
  name: "cargo-warm",
  strapline: "Private warm Cargo state for every worktree",
  description:
    "Fork useful Cargo and rustc state into isolated worktree caches so developers and coding agents can start closer to warm without sharing mutable build state.",
  repoUrl: "https://github.com/amxv/cargo-warm",
  accentColor: "#b7410e",
  accentColorDark: "#f28c52",
  footerSections: [
    {
      title: "cargo-warm",
      text: "Warm starting points for Rust worktrees, with a separate writable cache for every checkout."
    },
    {
      title: "Start",
      linkPrefix: "Read: ",
      linkHref: "/docs/quickstart",
      linkLabel: "Quickstart"
    },
    {
      title: "Repository",
      linkPrefix: "Source: ",
      linkHref: "https://github.com/amxv/cargo-warm",
      linkLabel: "github.com/amxv/cargo-warm"
    }
  ]
} as const;

export const docCategories = ["Start", "Integrate", "Concepts", "Operate", "Reference"] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
