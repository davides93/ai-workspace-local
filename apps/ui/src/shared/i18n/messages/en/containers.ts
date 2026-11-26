// English messages of "containers". One entry per line; must have exactly the keys of the Italian file.
import type { containersIt } from "../it/containers";

export const containersEn: Record<keyof typeof containersIt, string> = {
  "containers.voce_menu": "A computer for your agents",
  "containers.titolo": "A computer for your agents",
  "containers.intro": "Coworkers that work on their own need a place of their own, apart from your files: a container. Here I check whether this computer can host it.",
  "containers.errore": "I can't check Docker: {error}",
  "containers.riprova": "Try again",
  "containers.controlla_ancora": "Check again",
  "containers.controllo_automatico": "I check by myself every few seconds: when you are done, this page updates.",
  "containers.pronto_titolo": "{name} is ready",
  "containers.pronto_testo": "{name} {version} is running and has everything needed{resources}. I changed nothing on this computer.",
  "containers.fermo_titolo": "{name} is installed but off",
  "containers.fermo_testo": "Open the Docker Desktop app and wait until it says it is running.",
  "containers.fermo_testo_linux": "Start the Docker service (usually with “sudo systemctl start docker”) or the Podman machine.",
  "containers.manca_titolo": "Docker isn't here yet",
  "containers.manca_testo": "Install Docker Desktop, open it once and come back. It is free for personal use.",
  "containers.manca_testo_linux": "Install Docker Engine (or Podman), start it and come back.",
  "containers.manca_link": "Download Docker Desktop",
  "containers.manca_link_linux": "How to install Docker Engine",
  "containers.compose_titolo": "“Compose” is missing",
  "containers.compose_testo": "{name} is running but without the “Compose” component, which starts the pieces of the agent's computer together.",
  "containers.compose_link": "How to install Docker Compose",
  "containers.telefono_titolo": "Containers live on the computer",
  "containers.telefono_testo": "A phone can't run Docker. If you pair this phone with a computer that has Docker, your coworkers work there.",
} as const;
