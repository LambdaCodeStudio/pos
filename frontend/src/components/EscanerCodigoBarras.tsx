// Escáner de código de barras por cámara, para cargar productos desde el
// celular en /recepcion cuando no hay un lector físico conectado.

import { useEffect, useRef, useState } from 'react';
import { BrowserMultiFormatReader } from '@zxing/browser';
import { Modal, MensajeError } from './ui';

export default function EscanerCodigoBarras({
  onDetectado,
  onCerrar,
}: {
  onDetectado: (codigo: string) => void;
  onCerrar: () => void;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [error, setError] = useState<string | null>(null);
  // Refs para no reiniciar la cámara si el padre re-renderiza con una función nueva.
  const onDetectadoRef = useRef(onDetectado);
  onDetectadoRef.current = onDetectado;

  useEffect(() => {
    const lector = new BrowserMultiFormatReader();
    let controles: { stop: () => void } | undefined;
    let detectado = false;

    lector
      .decodeFromVideoDevice(undefined, videoRef.current!, (resultado, _err, ctrl) => {
        controles = ctrl;
        if (detectado || !resultado) return;
        detectado = true;
        ctrl.stop();
        onDetectadoRef.current(resultado.getText());
      })
      .catch((err) => {
        setError(err instanceof Error ? `No se pudo acceder a la cámara: ${err.message}` : 'No se pudo acceder a la cámara.');
      });

    return () => controles?.stop();
  }, []);

  return (
    <Modal abierto titulo="Escanear código de barras" onCerrar={onCerrar} ancho="max-w-md">
      <MensajeError error={error} />
      <div className="aspect-square overflow-hidden rounded-xl bg-stone-900">
        <video ref={videoRef} className="h-full w-full object-cover" muted playsInline />
      </div>
      <p className="mt-3 text-center text-sm text-stone-500">Apuntá la cámara al código de barras del producto.</p>
    </Modal>
  );
}
