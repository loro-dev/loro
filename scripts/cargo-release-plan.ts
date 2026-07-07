export type DependencyUpdate = {
  dependency: string;
  dependent: string;
};

export type ReleasePlan = {
  noChangesCrates: string[];
  plannedCrates: string[];
  dependencyUpdates: DependencyUpdate[];
};

export function parseReleasePlan(output: string): ReleasePlan {
  const lines = output.split("\n");
  const noChangesCrates: string[] = [];
  const plannedCrates: string[] = [];
  const dependencyUpdates: DependencyUpdate[] = [];
  let currentUpgradedCrate: string | undefined;

  for (const line of lines) {
    if (line.includes("despite no changes made since tag")) {
      const match = line.match(/updating ([^ ]+) to/);
      if (match) {
        noChangesCrates.push(match[1]);
      }
    }

    const upgradeMatch = line.match(/Upgrading ([^ ]+) from/);
    if (upgradeMatch) {
      currentUpgradedCrate = upgradeMatch[1];
      plannedCrates.push(currentUpgradedCrate);
      continue;
    }

    const dependencyMatch = line.match(/Updating ([^']+)'s dependency from/);
    if (dependencyMatch && currentUpgradedCrate) {
      dependencyUpdates.push({
        dependency: currentUpgradedCrate,
        dependent: dependencyMatch[1],
      });
    }
  }

  return { noChangesCrates, plannedCrates, dependencyUpdates };
}

export function selectExcludedCrates(plan: ReleasePlan): string[] {
  const noChangesCrates = new Set(plan.noChangesCrates);
  const requiredCrates = new Set(
    plan.plannedCrates.filter((crate) => !noChangesCrates.has(crate)),
  );

  let changed = true;
  while (changed) {
    changed = false;
    for (const update of plan.dependencyUpdates) {
      if (
        requiredCrates.has(update.dependency) &&
        noChangesCrates.has(update.dependent) &&
        !requiredCrates.has(update.dependent)
      ) {
        requiredCrates.add(update.dependent);
        changed = true;
      }
    }
  }

  return plan.noChangesCrates.filter((crate) => !requiredCrates.has(crate));
}
