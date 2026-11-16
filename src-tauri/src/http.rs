//! Cliente HTTP compartido por todas las fuentes, la redacción y la publicación.

use std::sync::LazyLock;
use std::time::Duration;

static CLIENTE: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(8))
        .user_agent("migas/0.1")
        // Sin redirecciones: una cabecera de autenticación nunca debe viajar a otro origen.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("cliente HTTP")
});

pub fn cliente() -> reqwest::Client {
    CLIENTE.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    #[tokio::test]
    async fn no_sigue_redirecciones_a_otro_origen() {
        let destino = MockServer::start();
        let robado = destino.mock(|when, then| {
            when.method(GET).path("/robado");
            then.status(200).json_body(json!({"ok": true}));
        });
        let origen = MockServer::start();
        let redireccion = origen.mock(|when, then| {
            when.method(GET).path("/inicio");
            then.status(302)
                .header("location", format!("{}/robado", destino.base_url()));
        });

        let respuesta = cliente()
            .get(format!("{}/inicio", origen.base_url()))
            .send()
            .await
            .expect("respuesta");
        redireccion.assert_calls(1);
        assert_eq!(respuesta.status().as_u16(), 302);
        assert_eq!(robado.calls(), 0, "el destino no debe recibir la petición");
    }
}
