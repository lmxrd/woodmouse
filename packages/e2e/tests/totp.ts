import { createHmac } from "node:crypto";

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const STEP_MS = 30_000;

function base32(secret: string): Buffer {
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const char of secret.replace(/=+$/, "").toUpperCase()) {
    value = (value << 5) | ALPHABET.indexOf(char);
    bits += 5;
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff);
      bits -= 8;
    }
  }
  return Buffer.from(out);
}

/** The six-digit code (RFC 6238: SHA-1, 30-second steps) of a base32 secret at a time step. */
export function codeAt(secret: string, step: number): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const hash = createHmac("sha1", base32(secret)).update(counter).digest();
  const at = hash[hash.length - 1]! & 0xf;
  return ((hash.readUInt32BE(at) & 0x7fffffff) % 1_000_000).toString().padStart(6, "0");
}

/**
 * An authenticator app: a code for the current step, waiting for the next step when the current one was
 * used already (the server accepts each step once).
 */
export class Authenticator {
  private lastStep = -1;

  constructor(readonly secret: string) {}

  async code(): Promise<string> {
    let step = Math.floor(Date.now() / STEP_MS);
    if (step <= this.lastStep) {
      await new Promise((resolve) =>
        setTimeout(resolve, (this.lastStep + 1) * STEP_MS - Date.now() + 250),
      );
      step = Math.floor(Date.now() / STEP_MS);
    }
    this.lastStep = step;
    return codeAt(this.secret, step);
  }
}
