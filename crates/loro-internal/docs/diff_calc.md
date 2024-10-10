# Internal of Diff Calculation

Diff calculation is the core of the `diff` command. It is responsible for calculating the difference between two versions of a container.

# Three modes of diff calculation

## 1. Checkout Mode

This is the most general mode of diff calculation. It can be used whenever a user want to switch to a different version.
But it is also the slowest mode. It relies on the `ContainerHistoryCache`, which is expensive to build and maintain in memory.

## 2. Import Mode

This mode is used when the user imports new updates. It is faster than the checkout mode, but it is still slower than the linear mode.

- The difference between the import mode and the checkout mode: in import mode, target version > current version.
  So when calculating the `DiffCalculator` doesn't need to rely on `ContainerHistoryCache`, except for the Tree container.
- The difference between the import mode and the linear mode: in linear mode, all the imported updates are ordered, no concurrent update exists.
  so there is no need to build CRDTs for the calculation

## 3. Linear Mode

This mode is used when we don't need to build CRDTs to calculate the difference. It is the fastest mode.
