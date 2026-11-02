import { useState, type FormEvent, type RefObject } from "react";
import { Button, Field, Input } from "../../../shared/ui";
import { digitsOf, validatePair, type PairInput } from "../model/mesh";

/** Pair with a device by typing its address and the code it shows. */
export function PairForm({ address, code, codeRef, pending, onAddress, onCode, onSubmit }: {
  address: string;
  code: string;
  codeRef: RefObject<HTMLInputElement | null>;
  pending: boolean;
  onAddress: (v: string) => void;
  onCode: (v: string) => void;
  onSubmit: (input: PairInput) => void;
}) {
  const [submitted, setSubmitted] = useState(false);
  const errors = submitted ? validatePair({ address, code }) : {};
  const submit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (Object.keys(validatePair({ address, code })).length === 0) onSubmit({ address: address.trim(), code: digitsOf(code) });
  };
  return (
    <form className="panel-body pair-form" onSubmit={submit} noValidate aria-label="Accoppia per indirizzo">
      <Field label="Indirizzo" htmlFor="peer-address" error={errors.address} className="grow">
        <Input id="peer-address" mono placeholder="192.168.1.20:7777" value={address} onChange={(e) => onAddress(e.target.value)} autoComplete="off" aria-invalid={!!errors.address} />
      </Field>
      <Field label="Codice" htmlFor="peer-code" error={errors.code}>
        <input
          id="peer-code" ref={codeRef} className="input mono" inputMode="numeric" placeholder="123 456" maxLength={7} value={code}
          onChange={(e) => onCode(e.target.value)} autoComplete="one-time-code" aria-invalid={!!errors.code}
        />
      </Field>
      <Button variant="primary" type="submit" disabled={pending}>{pending ? "Accoppio…" : "Accoppia"}</Button>
    </form>
  );
}
