use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
    pub jti: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRole {
    Viewer,
    Operator,
    Admin,
}

impl UserRole {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "viewer" => Some(Self::Viewer),
            "operator" => Some(Self::Operator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
    
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
    
    pub fn can_manage_jobs(&self) -> bool {
        matches!(self, Self::Operator | Self::Admin)
    }
    
    pub fn can_stop_jobs(&self) -> bool {
        matches!(self, Self::Operator | Self::Admin)
    }
    
    pub fn can_manage_nodes(&self) -> bool {
        matches!(self, Self::Admin)
    }
    
    pub fn can_manage_users(&self) -> bool {
        matches!(self, Self::Admin)
    }
    
    pub fn can_manage_rules(&self) -> bool {
        matches!(self, Self::Admin)
    }
    
    pub fn can_read_metrics(&self) -> bool {
        true
    }
    
    pub fn can_read_history(&self) -> bool {
        true
    }
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::auth(format!("Failed to hash password: {}", e)))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let argon2 = Argon2::default();
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::auth(format!("Invalid password hash: {}", e)))?;
    
    argon2.verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|e| AppError::auth(format!("Password verification failed: {}", e)))?;
    
    Ok(())
}

pub fn generate_jwt(user_id: &str, role: &str, secret: &str, access_expiry_secs: u64) -> Result<String, AppError> {
    let now = Utc::now();
    let exp = (now + ChronoDuration::seconds(access_expiry_secs as i64)).timestamp() as usize;
    let iat = now.timestamp() as usize;
    
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        exp,
        iat,
        jti: Uuid::new_v4().to_string(),
    };
    
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .map_err(|e| AppError::auth(format!("Failed to encode JWT: {}", e)))
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|e| AppError::unauthorized(format!("Invalid JWT: {}", e)))
}

pub fn generate_refresh_token() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_password_hash_and_verify() {
        let password = "test-password-123";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).is_ok());
        assert!(verify_password("wrong-password", &hash).is_err());
    }
    
    #[test]
    fn test_jwt_roundtrip() {
        let secret = "test-secret-key";
        let token = generate_jwt("user-1", "admin", secret, 3600).unwrap();
        let claims = verify_jwt(&token, secret).unwrap();
        
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.role, "admin");
        assert!(claims.exp > claims.iat);
    }
    
    #[test]
    fn test_jwt_invalid_secret() {
        let token = generate_jwt("user-1", "admin", "secret-a", 3600).unwrap();
        assert!(verify_jwt(&token, "secret-b").is_err());
    }
    
    #[test]
    fn test_role_permissions() {
        assert!(UserRole::Viewer.can_read_metrics());
        assert!(!UserRole::Viewer.can_manage_jobs());
        
        assert!(UserRole::Operator.can_manage_jobs());
        assert!(UserRole::Operator.can_stop_jobs());
        assert!(!UserRole::Operator.can_manage_nodes());
        
        assert!(UserRole::Admin.can_manage_nodes());
        assert!(UserRole::Admin.can_manage_users());
        assert!(UserRole::Admin.can_manage_rules());
    }
}
