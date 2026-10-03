/** Own asynchronous event subscriptions for one effect, including late arrivals. */
export function createSubscriptionScope(onError: (error: unknown) => void) {
  let disposed = false;
  const cleanups = new Set<() => void>();

  return {
    async add(subscription: Promise<() => void>): Promise<void> {
      try {
        const cleanup = await subscription;
        if (disposed) {
          try {
            cleanup();
          } catch (error) {
            onError(error);
          }
        }
        else cleanups.add(cleanup);
      } catch (error) {
        if (!disposed) onError(error);
      }
    },
    dispose() {
      disposed = true;
      for (const cleanup of cleanups) {
        try {
          cleanup();
        } catch (error) {
          onError(error);
        }
      }
      cleanups.clear();
    },
  };
}
