//! Error del crate: mensajes cortos en español, nunca con SQL ni rutas de credenciales.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Fallo de SQLite. El mensaje público no incluye la sentencia.
    #[error("Error de base de datos")]
    Bd(#[from] rusqlite::Error),
    /// Fallo de JSON (huellas, ajustes, mensajes del host).
    #[error("Error de datos")]
    Json(#[from] serde_json::Error),
    /// Dato de entrada inválido.
    #[error("{0}")]
    Validacion(String),
    /// La entrada o el recurso pedido no existe. Se pasa el complemento: `la entrada 12`.
    #[error("No existe {0}")]
    NoExiste(String),
    /// Fallo de fichero.
    #[error("Error de fichero")]
    Io(#[from] std::io::Error),
}

pub type Resultado<T> = Result<T, Error>;

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mensajes_cortos_en_espanol() {
        assert_eq!(
            Error::Validacion("Falta la URL".into()).to_string(),
            "Falta la URL"
        );
        assert_eq!(
            Error::NoExiste("la entrada 12".into()).to_string(),
            "No existe la entrada 12"
        );
        let bd = rusqlite::Connection::open_in_memory()
            .and_then(|c| c.execute("SELECT * FROM tabla_que_no_existe", []))
            .expect_err("la tabla no existe");
        let error = Error::Bd(bd);
        assert_eq!(error.to_string(), "Error de base de datos");
        assert!(!error.to_string().contains("tabla_que_no_existe"));
        let cadena: String = error.into();
        assert_eq!(cadena, "Error de base de datos");
    }
}
