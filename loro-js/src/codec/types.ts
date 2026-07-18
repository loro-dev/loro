export const ContainerType = {
  Map: "map",
  List: "list",
  Text: "text",
  Tree: "tree",
  MovableList: "movable-list",
  Counter: "counter",
} as const;

export type KnownContainerType = (typeof ContainerType)[keyof typeof ContainerType];

export interface UnknownContainerType {
  readonly kind: "unknown";
  readonly value: number;
}

export type ContainerType = KnownContainerType | UnknownContainerType;

export interface Id {
  readonly peer: bigint;
  readonly counter: number;
}

export interface RootContainerId {
  readonly kind: "root";
  readonly name: string;
  readonly containerType: ContainerType;
}

export interface NormalContainerId extends Id {
  readonly kind: "normal";
  readonly containerType: ContainerType;
}

export type ContainerId = RootContainerId | NormalContainerId;

export type VersionVector = readonly Id[];
export type Frontiers = readonly Id[];

export type EncodedLoroValue =
  | { readonly type: "null" }
  | { readonly type: "bool"; readonly value: boolean }
  | { readonly type: "double"; readonly value: number }
  | { readonly type: "i64"; readonly value: bigint }
  | { readonly type: "string"; readonly value: string }
  | { readonly type: "list"; readonly value: readonly EncodedLoroValue[] }
  | {
      readonly type: "map";
      readonly value: readonly (readonly [string, EncodedLoroValue])[];
    }
  | { readonly type: "container"; readonly value: ContainerId }
  | { readonly type: "binary"; readonly value: Uint8Array };
