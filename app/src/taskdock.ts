/**
 * Which task's layout the dock is showing, and who it gets saved for.
 *
 * Switching tasks clears the dock and loads another layout; both fire layout-change events, and a debounced save
 * that read "the current task" when it ran could write an empty or the other task's layout into the wrong file.
 * So the dock has an owner: changes are ignored while there is none (mid-switch), and a save is always for the
 * task that owned the dock when the change happened.
 */

export interface DockPort {
  toJSON(): string;
  clear(): void;
  /** Throws on a layout it cannot read. */
  fromJSON(json: string): void;
  openAgents(names: readonly string[]): void;
  /** Resolves once the layout-change events caused by the calls above have been delivered. */
  settled(): Promise<void>;
}

export interface LayoutStore {
  load(taskId: string): Promise<string | null>;
  save(taskId: string, json: string): Promise<void>;
}

const SAVE_DELAY_MS = 500;

export class TaskDock {
  private owner: string | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private writes: Promise<void> = Promise.resolve();
  /** Switches run one after another; a click during a switch waits its turn. */
  private queue: Promise<void> = Promise.resolve();

  constructor(
    private readonly dock: DockPort,
    private readonly store: LayoutStore,
    private readonly onError: (e: unknown) => void,
    private readonly delayMs = SAVE_DELAY_MS,
  ) {}

  /** The task whose layout is on screen; null while switching or when no task is shown. */
  get shown(): string | null {
    return this.owner;
  }

  /** To be called on every layout change of the dock. */
  changed(): void {
    const id = this.owner;
    if (id === null) return;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      if (this.owner === id) this.enqueue(id, this.dock.toJSON());
    }, this.delayMs);
  }

  /** Save the shown task's layout, then show `taskId` with its saved layout, or all of `agents` as tabs. */
  show(taskId: string, agents: readonly string[]): Promise<void> {
    return this.run(async () => {
      await this.release(true);
      let json: string | null = null;
      try {
        json = await this.store.load(taskId);
      } catch (e) {
        this.onError(e);
      }
      if (json !== null) {
        try {
          this.dock.fromJSON(json);
        } catch (e) {
          this.onError(`布局没能恢复，已按标签重开: ${e instanceof Error ? e.message : String(e)}`);
          this.dock.clear();
          json = null;
        }
      }
      if (json === null) this.dock.openAgents(agents);
      await this.dock.settled();
      this.owner = taskId;
    });
  }

  /** Empty the dock without saving its layout, for a task that is being deleted. */
  detach(): Promise<void> {
    return this.run(async () => {
      await this.release(false);
    });
  }

  private run(step: () => Promise<void>): Promise<void> {
    const next = this.queue.then(step);
    this.queue = next.catch(this.onError);
    return next;
  }

  /** From here until the next `show` the dock belongs to nobody, so its changes are ignored. */
  private async release(save: boolean): Promise<void> {
    clearTimeout(this.timer);
    const id = this.owner;
    this.owner = null;
    if (id !== null && save) this.enqueue(id, this.dock.toJSON());
    this.dock.clear();
    await this.dock.settled();
    // The next task's layout may be in a file that is still being written.
    await this.writes;
  }

  private enqueue(id: string, json: string): void {
    this.writes = this.writes.then(() => this.store.save(id, json)).catch(this.onError);
  }
}
