import * as http from "node:http";
import { basename } from "node:path";

export interface DockerContainer {
  Id: string;
  Names: string[];
  Image: string;
  State: string;
  Status: string;
  Labels: Record<string, string>;
}

const SOCKET_PATH = "/var/run/docker.sock";

/**
 * Realiza una petición GET genérica al Docker Socket.
 */
function fetchDockerAPI(path: string): Promise<any> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        socketPath: SOCKET_PATH,
        path: path,
        method: "GET",
      },
      (res) => {
        let rawData = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          rawData += chunk;
        });
        res.on("end", () => {
          try {
            if (res.statusCode && res.statusCode >= 400) {
              reject(new Error(`Docker API Error: ${res.statusCode} - ${rawData}`));
              return;
            }
            const parsedData = JSON.parse(rawData);
            resolve(parsedData);
          } catch (e) {
            reject(e);
          }
        });
      }
    );

    req.on("error", (e) => {
      reject(e);
    });
    req.end();
  });
}

/**
 * Obtiene los servicios asociados al proyecto de docker compose en el CWD actual.
 */
export async function getComposeServices(cwdPath: string = process.cwd()): Promise<DockerContainer[]> {
  // Docker compose usa por defecto el nombre base de la carpeta como `project` 
  // (a menos que se sobreescriba, pero esto es el caso estándar).
  const projectName = basename(cwdPath).toLowerCase().replace(/[^a-z0-9_-]/g, "");

  // Las labels que añade compose
  const filters = JSON.stringify({
    label: [`com.docker.compose.project=${projectName}`],
  });

  const encodedFilters = encodeURIComponent(filters);
  const data = await fetchDockerAPI(`/containers/json?all=true&filters=${encodedFilters}`);
  return data as DockerContainer[];
}

/**
 * Ejecuta una acción sobre un contenedor (start, stop, etc).
 */
export function executeContainerAction(containerId: string, action: "start" | "stop" | "restart"): Promise<void> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        socketPath: SOCKET_PATH,
        path: `/containers/${containerId}/${action}`,
        method: "POST",
      },
      (res) => {
        if (res.statusCode === 204) {
          resolve();
        } else {
          let rawData = "";
          res.on("data", (c) => (rawData += c));
          res.on("end", () => reject(new Error(`Action failed: ${res.statusCode} ${rawData}`)));
        }
      }
    );
    req.on("error", reject);
    req.end();
  });
}
