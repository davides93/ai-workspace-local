// English messages of "pairing". One entry per line; must have exactly the keys of the Italian file.
import type { pairingIt } from "../it/pairing";

export const pairingEn: Record<keyof typeof pairingIt, string> = {
  "pairing.questo_carrier_non_supporta_il": "This transport does not support pairing",
  "pairing.dispositivo_accoppiato": "Device paired",
  "pairing.accoppia_questo_dispositivo": "Pair this device",
  "pairing.dispositivi": "Devices",
  "pairing.codice_di_pairing": "Pairing code",
  "pairing.accoppia": "Pair",
};
