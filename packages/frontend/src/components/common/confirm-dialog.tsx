import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";

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
 * One confirmation dialog for the whole app (shadcn's AlertDialog), asked
 * through `useConfirm`: `if (await confirm({...})) ...`, where the browser's
 * `confirm()` would be.
 */
export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  // Kept after closing, so the texts stay while the dialog fades out.
  const [options, setOptions] = useState<ConfirmOptions | null>(null);
  const resolve = useRef<(answer: boolean) => void>(() => {});
  const confirm = useCallback<Confirm>(
    (next) =>
      new Promise((done) => {
        // A confirmation still open is answered no.
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
      <AlertDialog open={open} onOpenChange={(next) => !next && answer(false)}>
        <AlertDialogContent className="rounded-none">
          <AlertDialogHeader>
            <AlertDialogTitle className="font-display text-xl">{options?.title}</AlertDialogTitle>
            {options?.description && (
              <AlertDialogDescription>{options.description}</AlertDialogDescription>
            )}
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => answer(false)}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant={options?.destructive ? "destructive" : "default"}
              onClick={() => answer(true)}
            >
              {options?.action}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </ConfirmContext.Provider>
  );
}

/** Ask for a confirmation; resolves to whether it was given. */
export function useConfirm(): Confirm {
  const confirm = useContext(ConfirmContext);
  if (!confirm) throw new Error("useConfirm needs a ConfirmProvider");
  return confirm;
}
