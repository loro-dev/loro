import { defineConfig, tierPresets } from "sponsorkit";

export default defineConfig({
  github: { login: "loro-dev", type: "organization" },
  renderer: "tiers",
  formats: ["svg"],
  width: 900,
  tiers: [
    { title: "Diamond", monthlyDollars: 1000, preset: tierPresets.xl },
    { title: "Gold", monthlyDollars: 250, preset: tierPresets.xl },
    { title: "Silver", monthlyDollars: 100, preset: tierPresets.large },
    { title: "Bronze", monthlyDollars: 50, preset: tierPresets.base },
    { title: "Backer", preset: tierPresets.base },
  ],
});
