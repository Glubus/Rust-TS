export type DamageEvent = {
  playerId: number;
  baseDamage: number;
  critical: boolean;
};

export type DamageResult = {
  playerId: number;
  playerName: string;
  damage: number;
  score: number;
};

export type OverlayState = {
  visible: boolean;
  messages: string[];
};
