//! Cálculo de precio final de venta a partir del costo de recepción.
//! Regla (ARQUITECTURA.md §4):
//!   base = costo                     (si el costo ya incluye IVA)
//!   base = costo × (1 + iva/100)    (si no lo incluye)
//!   precio_final = base × (1 + markup/100)
//! Todo en `Decimal`, redondeo a centavos SOLO al final.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::ErrorApi;

pub fn calcular_precio_final_centavos(
    costo_centavos: i64,
    costo_incluye_iva: bool,
    iva_pct: Decimal,
    markup_pct: Decimal,
) -> Result<i64, ErrorApi> {
    let costo = Decimal::from(costo_centavos);
    let base = if costo_incluye_iva {
        costo
    } else {
        costo * (Decimal::ONE + iva_pct / Decimal::ONE_HUNDRED)
    };
    let precio = base * (Decimal::ONE + markup_pct / Decimal::ONE_HUNDRED);
    precio
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .ok_or_else(|| ErrorApi::Validacion("el precio calculado desborda".into()))
}

/// Redondeo comercial del precio de venta: al múltiplo más cercano, con la
/// mitad hacia arriba (resto < multiplo/2 baja, >= multiplo/2 sube). Con
/// `multiplo_centavos` en 0 o 1 no hace nada. Un precio menor que medio
/// múltiplo queda sin redondear: jamás se devuelve $0 para un precio real.
pub fn redondear_a_multiplo_centavos(precio_centavos: i64, multiplo_centavos: i64) -> i64 {
    if multiplo_centavos <= 1 || precio_centavos <= 0 {
        return precio_centavos;
    }
    let redondeado =
        (precio_centavos + multiplo_centavos / 2) / multiplo_centavos * multiplo_centavos;
    if redondeado == 0 {
        precio_centavos
    } else {
        redondeado
    }
}

/// Costo unitario base CON IVA incluido, en centavos. Es lo que se guarda en
/// el historial de precios y en la proyección `costo_actual_centavos`, para
/// que los costos sean comparables entre proveedores que pasan precios con y
/// sin IVA. El ítem de recepción conserva el costo original y el flag.
pub fn normalizar_costo_con_iva_centavos(
    costo_centavos: i64,
    costo_incluye_iva: bool,
    iva_pct: Decimal,
) -> Result<i64, ErrorApi> {
    if costo_incluye_iva {
        return Ok(costo_centavos);
    }
    let base = Decimal::from(costo_centavos) * (Decimal::ONE + iva_pct / Decimal::ONE_HUNDRED);
    base.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .ok_or_else(|| ErrorApi::Validacion("el costo calculado desborda".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn costo_con_iva_solo_aplica_markup() {
        // costo $10,00 con IVA incluido, markup 40% → $14,00
        let precio =
            calcular_precio_final_centavos(1000, true, dec("21.00"), dec("40.00")).unwrap();
        assert_eq!(precio, 1400);
    }

    #[test]
    fn costo_sin_iva_aplica_iva_y_markup() {
        // costo $10,00 sin IVA, IVA 21%, markup 40% → 1000 × 1.21 × 1.40 = 1694
        let precio =
            calcular_precio_final_centavos(1000, false, dec("21.00"), dec("40.00")).unwrap();
        assert_eq!(precio, 1694);
    }

    #[test]
    fn redondea_solo_al_final() {
        // 333 × 1.105 × 1.35 = 496.744875 → 497 (un solo redondeo al final).
        // Redondeando en pasos intermedios daría 368 × 1.35 = 496.8 → otro resultado
        // según el orden; la regla exige exactitud hasta el final.
        let precio =
            calcular_precio_final_centavos(333, false, dec("10.50"), dec("35.00")).unwrap();
        assert_eq!(precio, 497);
    }

    #[test]
    fn medio_centavo_redondea_hacia_arriba() {
        // 250 × 1.10 = 275 con IVA; markup 50% → 412.5 → 413
        let precio =
            calcular_precio_final_centavos(250, false, dec("10.00"), dec("50.00")).unwrap();
        assert_eq!(precio, 413);
    }

    #[test]
    fn iva_exento_no_altera_base() {
        let precio = calcular_precio_final_centavos(1000, false, dec("0.00"), dec("40.00")).unwrap();
        assert_eq!(precio, 1400);
    }

    #[test]
    fn redondeo_comercial_al_multiplo() {
        // $4.630 con redondeo a $100 → $4.600 (resto 30 < 50, baja).
        assert_eq!(redondear_a_multiplo_centavos(463_000, 10_000), 460_000);
        // $4.650 → $4.700 (resto 50, sube).
        assert_eq!(redondear_a_multiplo_centavos(465_000, 10_000), 470_000);
        // $4.680 → $4.700 (resto 80 > 50, sube).
        assert_eq!(redondear_a_multiplo_centavos(468_000, 10_000), 470_000);
        // Sin redondeo configurado (0): el precio queda igual.
        assert_eq!(redondear_a_multiplo_centavos(463_000, 0), 463_000);
        // Un precio menor que medio múltiplo no se aplasta a $0.
        assert_eq!(redondear_a_multiplo_centavos(3_000, 10_000), 3_000);
        // Redondeo a $50: $4.630 → $4.650.
        assert_eq!(redondear_a_multiplo_centavos(463_000, 5_000), 465_000);
    }

    #[test]
    fn normalizacion_de_costo() {
        assert_eq!(
            normalizar_costo_con_iva_centavos(1000, true, dec("21.00")).unwrap(),
            1000
        );
        assert_eq!(
            normalizar_costo_con_iva_centavos(1000, false, dec("21.00")).unwrap(),
            1210
        );
        // 999 × 1.105 = 1103.895 → 1104
        assert_eq!(
            normalizar_costo_con_iva_centavos(999, false, dec("10.50")).unwrap(),
            1104
        );
    }
}
