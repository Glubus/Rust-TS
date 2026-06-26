const session = {
  id: "raid-night-01",
  ticks: 0,
};

export function tickSession(): number {
  session.ticks += 1;
  return session.ticks;
}

export function sessionSnapshot(): { id: string; ticks: number } {
  return {
    id: session.id,
    ticks: session.ticks,
  };
}
