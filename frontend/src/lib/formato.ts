// Formato regional argentino. Los montos del backend son centavos enteros.

const formatoPesos = new Intl.NumberFormat('es-AR', {
  style: 'currency',
  currency: 'ARS',
  minimumFractionDigits: 2,
});

export function pesos(centavos: number | null | undefined): string {
  if (centavos === null || centavos === undefined) return '—';
  return formatoPesos.format(centavos / 100);
}

/** "10.000" (NUMERIC del backend) → "10" · "0.475" → "0,475" */
export function cantidad(valor: string | number): string {
  const numero = typeof valor === 'string' ? parseFloat(valor) : valor;
  return new Intl.NumberFormat('es-AR', { maximumFractionDigits: 3 }).format(numero);
}

export function fecha(iso: string | null | undefined): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleDateString('es-AR', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

export function fechaHora(iso: string | null | undefined): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleString('es-AR', {
    day: '2-digit', month: '2-digit', year: '2-digit',
    hour: '2-digit', minute: '2-digit',
  });
}

/** Entrada de dinero del usuario ("1.694,50" o "1694.5") → centavos. */
export function aCentavos(texto: string): number | null {
  const limpio = texto.trim().replace(/\./g, '').replace(',', '.');
  if (limpio === '') return null;
  const valor = Number(limpio);
  if (!Number.isFinite(valor) || valor < 0) return null;
  return Math.round(valor * 100);
}

/**
 * Redondeo comercial al múltiplo más cercano (mitad hacia arriba), espejo de
 * la regla del backend. `multiplo` 0 o 1 = sin redondeo; un precio menor que
 * medio múltiplo queda igual (nunca $0).
 */
export function redondearComercial(centavos: number, multiplo: number): number {
  if (multiplo <= 1 || centavos <= 0) return centavos;
  const redondeado = Math.floor((centavos + multiplo / 2) / multiplo) * multiplo;
  return redondeado === 0 ? centavos : redondeado;
}

/** Centavos → texto editable "1694,50" */
export function desdeCentavos(centavos: number | null | undefined): string {
  if (centavos === null || centavos === undefined) return '';
  return (centavos / 100).toFixed(2).replace('.', ',');
}
