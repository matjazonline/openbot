use crate::app_error::{AppError, AppResult};

pub fn normalize_embedding(mut vector: [f32; 384]) -> AppResult<[f32; 384]> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= 0.0 || vector.iter().any(|value| !value.is_finite()) {
        return Err(AppError::ServiceUnavailable(
            "The local embedding model returned an invalid vector".into(),
        ));
    }
    for value in &mut vector {
        *value = (f64::from(*value) / norm) as f32;
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn knowledge_chunk_vectors_are_finite_nonzero_and_normalized() {
        let mut valid = [0.; 384];
        valid[0] = 3.;
        valid[1] = 4.;
        let result = normalize_embedding(valid).unwrap();
        assert!((result.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() - 1.).abs() < 1e-6);
        for bad in [0., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut vector = [0.; 384];
            vector[0] = bad;
            assert!(normalize_embedding(vector).is_err());
        }
    }
}
