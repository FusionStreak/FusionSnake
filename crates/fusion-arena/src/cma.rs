//! Separable (diagonal) CMA-ES.
//!
//! Learns a step size per dimension instead of a full covariance matrix, which
//! suits a few noisy, loosely coupled parameters and needs no linear algebra.

use crate::rules::Rng;

/// Optimizer state. Higher scores are better.
#[derive(Debug, Clone)]
pub struct Cma {
    pub mean: Vec<f64>,
    pub sigma: f64,
    variance: Vec<f64>,
    path_sigma: Vec<f64>,
    path_cov: Vec<f64>,
    weights: Vec<f64>,
    population: usize,
    generation: u32,
    mu_eff: f64,
    c_sigma: f64,
    d_sigma: f64,
    c_c: f64,
    c_1: f64,
    c_mu: f64,
    chi_n: f64,
}

/// Default population size for `dimensions` parameters.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)] // The logarithm of a dimension count is non-negative.
pub fn default_population(dimensions: usize) -> usize {
    4 + (3.0 * (dimensions as f64).ln()).floor() as usize
}

fn standard_normal(rng: &mut Rng) -> f64 {
    // Box-Muller. The offset keeps the logarithm's argument above zero.
    #[allow(clippy::cast_precision_loss)]
    let mut uniform = || (rng.next_u64() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
    let (u, v) = (uniform(), uniform());
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

impl Cma {
    /// Creates an optimizer centred on `mean`.
    ///
    /// # Panics
    /// Panics if `population` is below 2 or `mean` is empty.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Small counts convert exactly.
    pub fn new(mean: Vec<f64>, sigma: f64, population: usize) -> Self {
        assert!(population >= 2, "population must be at least 2");
        assert!(!mean.is_empty(), "mean must not be empty");
        let n = mean.len() as f64;
        let mu = population / 2;

        let raw: Vec<f64> = (0..mu)
            .map(|i| ((mu as f64) + 0.5).ln() - ((i + 1) as f64).ln())
            .collect();
        let total: f64 = raw.iter().sum();
        let weights: Vec<f64> = raw.iter().map(|w| w / total).collect();
        let mu_eff = 1.0 / weights.iter().map(|w| w * w).sum::<f64>();

        let c_sigma = (mu_eff + 2.0) / (n + mu_eff + 5.0);
        let d_sigma = 1.0 + 2.0 * (((mu_eff - 1.0) / (n + 1.0)).sqrt() - 1.0).max(0.0) + c_sigma;
        let c_c = 4.0 / (n + 4.0);
        // The diagonal model can learn faster than a full matrix.
        let separable = (n + 1.5) / 3.0;
        let c_1 = (separable * 2.0 / ((n + 1.3).powi(2) + mu_eff)).min(1.0);
        let c_mu = (separable * 2.0 * (mu_eff - 2.0 + 1.0 / mu_eff) / ((n + 2.0).powi(2) + mu_eff))
            .min(1.0 - c_1);
        let chi_n = n.sqrt() * (1.0 - 1.0 / (4.0 * n) + 1.0 / (21.0 * n * n));

        let dimensions = mean.len();
        Self {
            mean,
            sigma,
            variance: vec![1.0; dimensions],
            path_sigma: vec![0.0; dimensions],
            path_cov: vec![0.0; dimensions],
            weights,
            population,
            generation: 0,
            mu_eff,
            c_sigma,
            d_sigma,
            c_c,
            c_1,
            c_mu,
            chi_n,
        }
    }

    /// Samples one generation of candidates.
    pub fn ask(&self, rng: &mut Rng) -> Vec<Vec<f64>> {
        (0..self.population)
            .map(|_| {
                self.mean
                    .iter()
                    .zip(&self.variance)
                    .map(|(mean, variance)| {
                        mean + self.sigma * variance.sqrt() * standard_normal(rng)
                    })
                    .collect()
            })
            .collect()
    }

    /// Updates the distribution from scored candidates.
    ///
    /// # Panics
    /// Panics if the slices differ in length or a score is NaN.
    #[allow(clippy::cast_precision_loss)] // Small counts convert exactly.
    pub fn tell(&mut self, samples: &[Vec<f64>], scores: &[f64]) {
        assert_eq!(samples.len(), scores.len());
        let mut order: Vec<usize> = (0..samples.len()).collect();
        order.sort_by(|&a, &b| scores[b].partial_cmp(&scores[a]).expect("score is NaN"));

        let dimensions = self.mean.len();
        let previous = self.mean.clone();
        for (d, mean) in self.mean.iter_mut().enumerate() {
            *mean = self
                .weights
                .iter()
                .zip(&order)
                .map(|(weight, &index)| weight * samples[index][d])
                .sum();
        }

        let step: Vec<f64> = (0..dimensions)
            .map(|d| (self.mean[d] - previous[d]) / self.sigma)
            .collect();
        let sigma_rate = (self.c_sigma * (2.0 - self.c_sigma) * self.mu_eff).sqrt();
        for d in 0..dimensions {
            self.path_sigma[d] = (1.0 - self.c_sigma) * self.path_sigma[d]
                + sigma_rate * step[d] / self.variance[d].sqrt();
        }
        let path_norm = self.path_sigma.iter().map(|p| p * p).sum::<f64>().sqrt();

        self.generation += 1;
        let decay = 1.0 - (1.0 - self.c_sigma).powf(2.0 * f64::from(self.generation));
        let threshold = (1.4 + 2.0 / (dimensions as f64 + 1.0)) * self.chi_n;
        let stalled = path_norm / decay.sqrt() >= threshold;
        let h_sigma = if stalled { 0.0 } else { 1.0 };

        let cov_rate = (self.c_c * (2.0 - self.c_c) * self.mu_eff).sqrt();
        for d in 0..dimensions {
            self.path_cov[d] = (1.0 - self.c_c) * self.path_cov[d] + h_sigma * cov_rate * step[d];
            let selected: f64 = self
                .weights
                .iter()
                .zip(&order)
                .map(|(weight, &index)| {
                    let y = (samples[index][d] - previous[d]) / self.sigma;
                    weight * y * y
                })
                .sum();
            let correction = (1.0 - h_sigma) * self.c_c * (2.0 - self.c_c) * self.variance[d];
            self.variance[d] = (1.0 - self.c_1 - self.c_mu) * self.variance[d]
                + self.c_1 * (self.path_cov[d].powi(2) + correction)
                + self.c_mu * selected;
        }

        self.sigma *= ((self.c_sigma / self.d_sigma) * (path_norm / self.chi_n - 1.0)).exp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn optimize(score: impl Fn(&[f64]) -> f64, start: Vec<f64>, generations: u32) -> Vec<f64> {
        let population = default_population(start.len());
        let mut cma = Cma::new(start, 0.5, population);
        let mut rng = Rng::new(7);
        for _ in 0..generations {
            let samples = cma.ask(&mut rng);
            let scores: Vec<f64> = samples.iter().map(|x| score(x)).collect();
            cma.tell(&samples, &scores);
        }
        cma.mean
    }

    #[test]
    fn finds_the_optimum_of_a_sphere() {
        let target = [1.0, -2.0, 0.5, 3.0, -1.0, 2.0, 0.0, -0.5];
        let mean = optimize(
            |x| {
                -x.iter()
                    .zip(&target)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
            },
            vec![0.0; target.len()],
            250,
        );
        for (found, wanted) in mean.iter().zip(&target) {
            assert!((found - wanted).abs() < 1e-3, "{found} vs {wanted}");
        }
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn handles_badly_scaled_dimensions() {
        let mean = optimize(
            |x| {
                -x.iter()
                    .enumerate()
                    .map(|(i, v)| 10_f64.powi(i32::try_from(i).unwrap()) * (v - 1.0).powi(2))
                    .sum::<f64>()
            },
            vec![0.0; 5],
            500,
        );
        for value in mean {
            assert!((value - 1.0).abs() < 1e-3, "{value}");
        }
    }

    #[test]
    fn same_seed_gives_same_samples() {
        let cma = Cma::new(vec![0.0; 4], 0.3, 8);
        assert_eq!(cma.ask(&mut Rng::new(3)), cma.ask(&mut Rng::new(3)));
    }
}
