import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";

/** What a confirmation asks. `destructive` paints the action red. */
export type ConfirmOptions = {
  title: string;
  description?: string;
  action: string;
  destructive?: boolean;
};

type Confirm = (options: ConfirmOptions) => Promise<boolean>;

const ConfirmContext = createContext<Confirm | null>(null);

/**
 * One "are you sure" dialog for the whole app, asked through `useConfirm`:
 * `if (await confirm({...})) ...`, where the browser's `confirm()` would be.
 */
export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  // Kept after closing, so the texts stay while the dialog fades out.
  const [options, setOptions] = useState<ConfirmOptions | null>(null);
  const resolve = useRef<(answer: boolean) => void>(() => {});
  const confirm = useCallback<Confirm>(
    (next) =>
      new Promise((done) => {
        resolve.current(false);
        resolve.current = done;
        setOptions(next);
        setOpen(true);
      }),
    [],
  );
  const answer = (value: boolean) => {
    resolve.current(value);
    resolve.current = () => {};
    setOpen(false);
  };
  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      <Dialog open={open} onOpenChange={(next) => !next && answer(false)}>
        <DialogContent showCloseButton={false} className="rounded-none">
          <DialogHeader>
            <DialogTitle className="font-display text-xl">{options?.title}</DialogTitle>
            {options?.description && <DialogDescription>{options.description}</DialogDescription>}
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => answer(false)}>
              Keep
            </Button>
            <Button
              type="button"
              variant={options?.destructive ? "destructive" : "default"}
              onClick={() => answer(true)}
            >
              {options?.action}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </ConfirmContext.Provider>
  );
}

/** Ask for a confirmation; resolves to whether it was given. */
export function useConfirm(): Confirm {
  const confirm = useContext(ConfirmContext);
  if (!confirm) throw new Error("useConfirm needs a ConfirmProvider");
  return confirm;
}
