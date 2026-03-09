import * as http from "node:http";

const SOCKET_PATH = "/var/run/docker.sock";

export interface LogLine {
  streamType: "stdout" | "stderr";
  timestamp: Date;
  content: string;
}

/**
 * Conecta al endpoint de logs de Docker con multiplexing y llama a onLog.
 * Devuelve una función para abortar/cerrar la petición (unsubscribe).
 */
export function tailContainerLogs(
  containerId: string,
  onLog: (line: LogLine) => void,
  tail: string = "100" // Líneas de backlog a pedir inicialmente
): () => void {
  const req = http.request(
    {
      socketPath: SOCKET_PATH,
      path: `/containers/${containerId}/logs?follow=true&stdout=true&stderr=true&timestamps=true&tail=${tail}`,
      method: "GET",
    },
    (res) => {
      let buffer = Buffer.alloc(0);

      res.on("data", (chunk: Buffer) => {
        // Añadimos el nuevo chunk al buffer sobrante de la lectura anterior
        buffer = Buffer.concat([buffer, chunk]);

        // Docker multiplexed stream header:
        // [8 bytes]
        // byte 0: STREAM_TYPE (0 = stdin, 1 = stdout, 2 = stderr, 3 = system_err)
        // bytes 1-3: padding (0,0,0)
        // bytes 4-7: PAYLOAD_SIZE (Big Endian)
        
        while (buffer.length >= 8) {
          const streamTypeByte = buffer.readUInt8(0);
          const payloadSize = buffer.readUInt32BE(4);

          // Si aún no tenemos todos los datos del payload de este mensaje, esperamos al siguiente chunk
          if (buffer.length < 8 + payloadSize) {
            break;
          }

          const payload = buffer.subarray(8, 8 + payloadSize);
          // Avanzamos el buffer
          buffer = buffer.subarray(8 + payloadSize);

          const streamType = streamTypeByte === 2 ? "stderr" : "stdout";
          const messageStr = payload.toString("utf8");

          // Como pedimos timestamps=true, Docker precede cada linea con:
          // "2024-03-09T10:15:30.123456789Z el resto del log..."
          const spaceIdx = messageStr.indexOf(" ");
          if (spaceIdx > -1) {
            const timeStr = messageStr.substring(0, spaceIdx);
            const contentStr = messageStr.substring(spaceIdx + 1);
            
            try {
              const timestamp = new Date(timeStr);
              onLog({ streamType, timestamp, content: contentStr });
            } catch {
               // Fallback por si el timestamp fue invalido
              onLog({ streamType, timestamp: new Date(), content: messageStr });
            }
          } else {
             onLog({ streamType, timestamp: new Date(), content: messageStr });
          }
        }
      });
      
      res.on("error", (e) => console.error("Docked Logs Drop", e));
    }
  );

  req.on("error", (e) => {
    console.error(`Error connecting to tail logs for ${containerId}`, e);
  });
  
  req.end();

  return () => {
    // Para desuscribirse, abortamos la petición
    req.destroy();
  };
}
