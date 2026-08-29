export const siteConfig = {
  name: "cargo-warm",
  strapline: "A Rust CLI ready to ship",
  description:
    "Documentation for cargo-warm, a Rust command-line starter with tests, dist-powered releases, npm installation, and an isolated docs site.",
  repoUrl: "https://github.com/amxv/cargo-warm",
  accentColor: "#b7410e",
  accentColorDark: "#f28c52",
  footerSections: [
    {
      title: "cargo-warm",
      text: "A Rust CLI with build, release, installer, and documentation plumbing ready from the first commit."
    },
    {
      title: "Distribution",
      text: "GitHub Releases, shell and PowerShell installers, npm packages, checksums, and artifact attestations are driven by dist."
    },
    {
      title: "Repository",
      linkPrefix: "Source: ",
      linkHref: "https://github.com/amxv/cargo-warm",
      linkLabel: "github.com/amxv/cargo-warm"
    }
  ]
} as const;

export const docCategories = ["Start", "Development", "Distribution", "Reference"] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
