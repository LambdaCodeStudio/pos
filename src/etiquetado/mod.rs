pub mod rutas;

/// Formatea centavos como precio ya listo para imprimir en la etiqueta
/// térmica: pesos argentinos, separador de miles con punto, sin decimales si
/// los centavos son cero ($1.850), con coma decimal si no ($1.850,50). El
/// firmware imprime este string tal cual, sin procesarlo — el redondeo y el
/// signo son responsabilidad exclusiva de este helper.
pub fn formato_precio_centavos(centavos: i64) -> String {
    let centavos = centavos.max(0) as u64;
    let pesos = centavos / 100;
    let fraccion = centavos % 100;

    let pesos_str = pesos.to_string();
    let total = pesos_str.len();
    let mut con_miles = String::with_capacity(total + total / 3);
    for (i, c) in pesos_str.chars().enumerate() {
        if i > 0 && (total - i) % 3 == 0 {
            con_miles.push('.');
        }
        con_miles.push(c);
    }

    if fraccion == 0 {
        format!("${con_miles}")
    } else {
        format!("${con_miles},{fraccion:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::formato_precio_centavos;

    #[test]
    fn sin_centavos_no_muestra_decimales() {
        assert_eq!(formato_precio_centavos(185_000), "$1.850");
    }

    #[test]
    fn con_centavos_usa_coma_decimal() {
        assert_eq!(formato_precio_centavos(185_050), "$1.850,50");
    }

    #[test]
    fn menor_a_mil_sin_separador_de_miles() {
        assert_eq!(formato_precio_centavos(999), "$9,99");
    }

    #[test]
    fn cero() {
        assert_eq!(formato_precio_centavos(0), "$0");
    }

    #[test]
    fn monto_grande_con_varios_separadores() {
        assert_eq!(formato_precio_centavos(12_345_678_900), "$123.456.789");
    }

    #[test]
    fn centavos_con_cero_a_la_izquierda_se_completan() {
        assert_eq!(formato_precio_centavos(100_005), "$1.000,05");
    }
}
