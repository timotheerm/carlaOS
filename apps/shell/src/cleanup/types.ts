// Shapes returned by the Rust side (`carla_core::debloat` and `carla_core::admin`).

export type Tier = "recommended" | "optional" | "manual" | "kept";
export type Kind = "app" | "startup" | "service" | "setting";

export type Item = {
  id: string;
  kind: Kind;
  tier: Tier;
  title: string;
  detail: string;
  needs_admin: boolean;
};

export type Change = {
  item_id: string;
  title: string;
  kind: Kind;
  applied_at: number; // seconds since 1970
  needs_admin: boolean;
};

export type Report = { items: Item[]; changes: Change[]; warnings: string[] };

export type Outcome = {
  done: string[];
  failed: { id: string; title: string; error: string }[];
  restart: boolean;
};

export type AdminStatus = { pin_set: boolean; unlocked: boolean };

export type UnlockResult = { result: "unlocked" } | { result: "wrong" } | { result: "wait"; seconds: number };

export type Backend = {
  status(): Promise<AdminStatus>;
  setPin(pin: string): Promise<void>;
  unlock(pin: string): Promise<UnlockResult>;
  lock(): Promise<void>;
  scan(): Promise<Report>;
  apply(ids: string[]): Promise<Outcome>;
  undo(ids: string[]): Promise<Outcome>;
};
