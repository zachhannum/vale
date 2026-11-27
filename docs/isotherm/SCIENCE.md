# Isotherm: science

This document gives the equations of the climate model of Isotherm and the published source of each part. `docs/isotherm/DESIGN.md` gives the plan of the model. Each task that adds a part of the model adds its equations and its sources here.

The equations use SI units unless a line says otherwise. A constant with the subscript 0 is the value of the Earth preset. The free constants of each part are tuned against the Earth preset, and the tuned values go in the code next to the step that uses them.

## Rules for sources

The model comes from published papers, textbooks, and public data. The project can also use public information about other apps, such as a store page or a manual. It must not copy the code, the text, the names, or the art of another app or model.

The project must not copy code from these climate models. Their licenses are GPL or not clear. You can read their papers and cite them, but you must not read their code to write the code of Isotherm.

| Model | Paper |
| --- | --- |
| PlaSim | Fraedrich, K., Jansen, H., Kirk, E., Luksch, U., & Lunkeit, F. (2005). The Planet Simulator: Towards a user friendly model. Meteorologische Zeitschrift, 14(3), 299–304. https://doi.org/10.1127/0941-2948/2005/0043 |
| ExoPlaSim | Paradise, A., Macdonald, E., Menou, K., Lee, C., & Fan, B. (2022). ExoPlaSim: Extending the Planet Simulator for exoplanets. Monthly Notices of the Royal Astronomical Society, 511(3), 3272–3303. https://doi.org/10.1093/mnras/stac172 |
| Isca | Vallis, G. K., Colyer, G., Geen, R., Gerber, E., Jucker, M., Maher, P., Paterson, A., Pietschnig, M., Penn, J., & Thomson, S. I. (2018). Isca, v1.0: A framework for the global modelling of the atmospheres of Earth and other planets at varying levels of complexity. Geoscientific Model Development, 11(3), 843–859. https://doi.org/10.5194/gmd-11-843-2018 |

## Tier 1: energy balance

### Energy balance of a cell

Each cell has one surface temperature `T`. Its change in each step is the sum of the flows of energy into the cell.

```
C dT/dt = S (1 − α) − OLR + ∇·(D ∇h) + F_tide
```

`C` is the heat capacity of the cell, `S` is the insolation, `α` is the albedo, `OLR` is the outgoing infrared flux, `D` is the diffusivity, `h` is the moist static energy, and `F_tide` is the tidal heat flux from below.

Sources: North (1975), Budyko (1969), Sellers (1969), North, Cahalan, and Coakley (1981).

### Insolation

The orbit is a Kepler ellipse. The mean anomaly `M` grows at a fixed rate, and the eccentric anomaly `E` and the true anomaly `ν` follow from it.

```
M = E − e sin E
tan(ν/2) = sqrt((1 + e) / (1 − e)) tan(E/2)
r = a (1 − e cos E)
```

The declination `δ` of the star follows from the tilt `ε` and the true longitude `λ = ν + ϖ`, where `ϖ` is the longitude of the periapsis. The hour angle `H` follows from the longitude of the cell and the angle of the planet about its axis.

```
sin δ = sin ε sin λ
cos Z = sin φ sin δ + cos φ cos δ cos H
S = S_star (a_ref / r)² max(0, cos Z)
```

`φ` is the latitude, `Z` is the zenith angle, and `S_star` is the flux of the star at the distance `a_ref`. The model takes the mean of `S` over each step. The angle of the planet about its axis advances with the length of the solar day. A tidally locked world has a solar day of infinite length, so `H` does not change. A 3:2 resonance follows from the same equations.

Sources: Berger (1978), Hartmann (2016).

### Star spectrum and two bands

The star is a black body at the temperature `T_star`. The fraction `f_vis` of its energy below the wavelength `λ_c = 0.7 µm` is the integral of the Planck function to `λ_c`, divided by `σ T_star⁴`. The rest is the near-infrared band.

```
α = f_vis α_vis + (1 − f_vis) α_nir
```

Snow and ice have a high albedo in the visible band and a low albedo in the near-infrared band. A cool star puts more of its energy in the near-infrared band, so the ice of its planets absorbs more of the light.

Sources: Joshi and Haberle (2012), Shields et al. (2013).

### Binary stars

Each star of a binary gives its own `S`, from its own position and its own spectrum. The cell adds the two values, and each band adds the two shares.

Source: Forgan (2014).

### Gray atmosphere

The atmosphere has one optical depth `τ` for the infrared. The outgoing flux follows from the Eddington approximation of a gray atmosphere.

```
OLR = σ T⁴ / (1 + 3τ/4)
τ = (p_s / p_0)^n (τ_0 + k_CO2 ln(c_CO2 / c_CO2,0) + k_CH4 (sqrt(c_CH4) − sqrt(c_CH4,0)) + k_H2O q)
```

`p_s` is the surface pressure. The exponent `n` is between 1 and 2, because pressure broadening makes the lines wider. The terms in CO2 and CH4 follow the forms of the radiative forcing of Myhre et al. (1998). The term in water vapor uses the specific humidity `q` of the cell, so the water vapor feedback follows.

Sources: Pierrehumbert (2010), Frierson, Held, and Zurita-Gotor (2006), Myhre et al. (1998).

### Heat capacity

```
C_ocean = ρ_w c_w H_mix
C_land = ρ_s c_s H_soil
```

The slab ocean has a mixed layer of fixed depth `H_mix`, 50 m in the Earth preset. Land has a thin layer, 2 m in the Earth preset. Sea ice is a layer with no heat capacity of its own, as in the zero-layer model of Semtner. Its thickness changes with the net flux at its base and at its top.

```
ρ_i L_f dH_i/dt = F_base − F_top
F_top = k_i (T_f − T) / H_i
```

`T_f` is the freezing point of sea water, and `k_i` is the conductivity of ice.

Sources: Semtner (1976), Hartmann (2016).

### Transport of moist static energy

The heat transport is a diffusion of the moist static energy `h` on the sphere.

```
h = c_p T + L_v q
q = RH q_s(T)
q_s(T) = 0.622 e_s(T) / p_s
e_s(T) = 611.2 exp(17.67 (T − 273.15) / (T − 29.65))
```

`RH` is 0.8 in tier 1. The formula of `e_s` is the formula of Bolton (1980), in Pa. The diffusivity scales with the pressure, the heat capacity of the air, the molar mass, and the rotation rate.

```
D = D_0 (p_s / p_0) (c_p / c_p,0) (m_0 / m)² (Ω_0 / Ω)²
```

Sources: Hwang and Frierson (2010), Frierson, Held, and Zurita-Gotor (2007), Bolton (1980), Williams and Kasting (1997).

### Snow, ice, and albedo feedback

The albedo of snow and ice changes smoothly from the value of open ground or open water to the value of ice, near the freezing point.

```
α(T) = α_ice + (α_free − α_ice) (1 + tanh((T − T_f) / ΔT)) / 2
```

Sources: Budyko (1969), Spiegel, Menou, and Scharf (2008).

### Lapse rate

The model computes `T` at sea level. The temperature of a cell at the height `z` is lower by the lapse rate `Γ`.

```
T(z) = T − Γ z
Γ_dry = g / c_p
```

`Γ` is between the moist lapse rate and `Γ_dry`. It is 6.5 K per km in the Earth preset, and it scales with `g / c_p` on other planets.

Source: Hartmann (2016).

### Tidal heating

A moon on an eccentric orbit flexes the planet. The model spreads the tidal heat of the planet evenly over its surface.

```
Ė = (21/2) (k_2 / Q) G m² R⁵ n e² / a⁶
F_tide = Ė / (4π R²)
```

`m`, `n`, `e`, and `a` are the mass, the mean motion, the eccentricity, and the semimajor axis of the moon. `R` is the radius of the planet, `k_2` is its Love number, and `Q` is its tidal quality factor.

Source: Murray and Dermott (1999).

## Tier 2: circulation and water

### Surface pressure and winds

The surface pressure has a part from the temperature and a part from the Hadley cell and the subtropical highs. A warm area has low pressure, as in the boundary layer model of Lindzen and Nigam (1987).

```
p = p_cell(φ) − ρ_a H_bl g (T − T_ref) / T_ref
```

`p_cell` is a pressure profile with low pressure at the rising branch near the equator and high pressure at the poleward edge of the Hadley cell. The edge of the Hadley cell follows Held and Hou (1980).

```
φ_H = sqrt((5/3) g H_t Δ_h / (Ω² a²))
```

`H_t` is the height of the tropopause, `Δ_h` is the fractional difference of potential temperature from the equator to the pole, `Ω` is the rotation rate, and `a` is the radius of the planet. The rising branch follows the latitude of the highest temperature through the year, as Lindzen and Hou (1988) describe. Frierson, Lu, and Chen (2007) give the change of the width in a model with eddies.

The surface wind is a balance of the pressure force, the Coriolis force, and a linear friction.

```
f k × u = −∇p / ρ_a − r u
u = (−r ∇p + f k × ∇p) / (ρ_a (r² + f²))
```

`f = 2Ω sin φ` is the Coriolis parameter, and `r` is the friction rate. Land has a higher `r` than the sea. Near the equator, `f` is small, and the wind flows down the pressure gradient.

Sources: Lindzen and Nigam (1987), Held and Hou (1980), Lindzen and Hou (1988), Frierson, Lu, and Chen (2007), Holton and Hakim (2013).

### Regimes of rotation

The thermal Rossby number `Ro_T` chooses the regime of the circulation.

```
Ro_T = g H_t Δ_h / (Ω a)²
```

If `Ro_T` is small, the planet has Hadley cells, trade winds, and westerlies. If `Ro_T` is near 1 or larger, a Hadley cell spans each hemisphere, and the temperature difference from the equator to the pole is small. A tidally locked world has a day-night regime. The air rises over the point under the star and flows to the night side at height. Near the surface, it flows back toward the point under the star. A thick cloud deck forms over the point under the star.

Sources: Showman et al. (2013), Merlis and Schneider (2010), Pierrehumbert (2011), Yang, Cowan, and Abbot (2013).

### Humidity and evaporation

The winds carry the specific humidity `q` of the air.

```
∂q/∂t + u·∇q = E − P + ∇·(K ∇q)
E = ρ_a C_E |u| β (q_s(T) − q)
```

`C_E` is the bulk transfer coefficient, and `β` is the wetness of the surface. `β` is 1 over the sea. Over land, `β` follows the bucket model of Manabe (1969).

Sources: Manabe (1969), Hartmann (2016).

### Precipitation

Water falls when the air saturates. The air saturates sooner where the wind climbs a slope, because the air cools as it rises.

```
P = max(0, q − q_s(T(z))) / τ_P + P_oro
P_oro = max(0, c_oro q u·∇z_s)
```

`z_s` is the height of the ground, and `τ_P` is the time scale of rain. The term `P_oro` is the upslope source of the linear theory of Smith and Barstad (2004), without the delay of the fall of the water. The air that leaves the slope is dry, so a rain shadow forms on the far side.

Sources: Smith and Barstad (2004), Roe (2005).

### Rain and snow

The share of snow changes linearly from 1 to 0 between `T_s − 1 K` and `T_s + 1 K`, at the height of the cell. `T_s` is 1 °C in the Earth preset.

Source: Jennings et al. (2018).

### Soil water and runoff

Each land cell has a bucket of soil water `W` with the capacity `W_fc`, 150 mm in the Earth preset.

```
dW/dt = P_rain + M_snow − E
β = min(1, W / (0.75 W_fc))
R = max(0, W − W_fc) / Δt
```

`M_snow` is the melt of the snow cover, and `R` is the runoff. The runoff goes to the rivers of the flow network of `vale-terrain`.

Source: Manabe (1969).

### Ocean currents

The wind stress drives the currents.

```
τ_w = ρ_a C_D |u| u
M_E = τ_w × k / (ρ_w f)
β_R V = k · ∇ × (τ_w / ρ_w)
r_o ∇²ψ + β_R ∂ψ/∂x = k · ∇ × τ_w / (ρ_w H)
```

`M_E` is the Ekman transport, at a right angle to the wind. `β_R = df/dy` is the change of the Coriolis parameter with latitude, and `V` is the northward Sverdrup transport. The model solves the Stommel equation for the streamfunction `ψ` in each basin. Its friction term `r_o` makes the currents strong at the western edge of each basin. Near the equator, `f` is small, and the model limits the Ekman transport.

The currents carry the heat of the mixed layer.

```
C_ocean (∂T/∂t + u_o·∇T) = … + ∇·(K_o ∇T)
```

Sources: Ekman (1905), Sverdrup (1947), Stommel (1948), Vallis (2017).

### Clouds

The cloud fraction follows from the relative humidity and the vertical motion.

```
c = clamp(((RH − RH_c) / (1 − RH_c))², 0, 1)
```

`RH_c` is lower where the air rises and higher where it sinks. Clouds add to the albedo of the visible band and the near-infrared band, and they add to `τ`.

Source: Slingo (1987).

### Köppen-Geiger classes

The classes follow the thresholds of Peel, Finlayson, and McMahon (2007). Temperatures are in °C and precipitation is in mm. `T_hot` and `T_cold` are the means of the warmest month and the coldest month, `MAT` is the mean yearly temperature, and `MAP` is the yearly precipitation.

```
P_th = 2 MAT       if 70 % or more of MAP falls in winter
P_th = 2 MAT + 28  if 70 % or more of MAP falls in summer
P_th = 2 MAT + 14  otherwise

B:  MAP < 10 P_th        (BW if MAP < 5 P_th, else BS)
A:  not B, T_cold ≥ 18
C:  not B, T_hot > 10, 0 < T_cold < 18
D:  not B, T_hot > 10, T_cold ≤ 0
E:  not B, T_hot ≤ 10    (ET if T_hot > 0, else EF)
```

The Earth preset compares its classes with the maps of Beck et al. (2023), on the terrain of ETOPO 2022.

Sources: Peel, Finlayson, and McMahon (2007), Beck et al. (2023), NOAA National Centers for Environmental Information (2022).

## Observations for the checks

The checks of the Earth preset compare its outputs with these data sets. `docs/isotherm/DESIGN.md` lists the checks under "Correctness". Each data set gives the means of 1991 to 2020.

| Data set | License | Outputs |
| --- | --- | --- |
| ERA5 monthly averaged data on single levels (Copernicus Climate Change Service, 2019), described in Hersbach et al. (2020) | CC BY | Surface air temperature, sea ice, sea-level pressure, surface wind, cloud cover, sea surface temperature |
| GPCP monthly precipitation, version 2.3 (Adler et al., 2017), described in Adler et al. (2018) | CC0 | Precipitation |
| Drifter climatology of near-surface currents (Laurindo, Mariano, and Lumpkin, 2017) | Not clear, to be confirmed before use | Ocean currents |
| Köppen-Geiger maps of Beck et al. (2023) | CC BY 4.0 | Climate classes |

## References

Adler, R., Wang, J.-J., Sapiano, M., Huffman, G., Chiu, L., Xie, P.-P., Ferraro, R., Schneider, U., Becker, A., Bolvin, D., Nelkin, E., Gu, G., & NOAA CDR Program. (2017). Global Precipitation Climatology Project (GPCP) Climate Data Record (CDR), Version 2.3 (Monthly). NOAA National Centers for Environmental Information. https://doi.org/10.7289/V56971M6

Adler, R. F., Sapiano, M. R. P., Huffman, G. J., Wang, J.-J., Gu, G., Bolvin, D., Chiu, L., Schneider, U., Becker, A., Nelkin, E., Xie, P., Ferraro, R., & Shin, D.-B. (2018). The Global Precipitation Climatology Project (GPCP) monthly analysis (new version 2.3) and a review of 2017 global precipitation. Atmosphere, 9(4), 138. https://doi.org/10.3390/atmos9040138

Beck, H. E., McVicar, T. R., Vergopolan, N., Berg, A., Lutsko, N. J., Dufour, A., Zeng, Z., Jiang, X., van Dijk, A. I. J. M., & Miralles, D. G. (2023). High-resolution (1 km) Köppen-Geiger maps for 1901–2099 based on constrained CMIP6 projections. Scientific Data, 10, 724. https://doi.org/10.1038/s41597-023-02549-6

Berger, A. L. (1978). Long-term variations of daily insolation and Quaternary climatic changes. Journal of the Atmospheric Sciences, 35(12), 2362–2367. https://doi.org/10.1175/1520-0469(1978)035<2362:ltvodi>2.0.co;2

Bolton, D. (1980). The computation of equivalent potential temperature. Monthly Weather Review, 108(7), 1046–1053. https://doi.org/10.1175/1520-0493(1980)108<1046:tcoept>2.0.co;2

Budyko, M. I. (1969). The effect of solar radiation variations on the climate of the Earth. Tellus, 21(5), 611–619. https://doi.org/10.1111/j.2153-3490.1969.tb00466.x

Copernicus Climate Change Service. (2019). ERA5 monthly averaged data on single levels from 1940 to present. Copernicus Climate Change Service (C3S) Climate Data Store (CDS). https://doi.org/10.24381/cds.f17050d7

Ekman, V. W. (1905). On the influence of the earth's rotation on ocean-currents. Arkiv för Matematik, Astronomi och Fysik, 2(11), 1–52.

Forgan, D. (2014). Assessing circumbinary habitable zones using latitudinal energy balance modelling. Monthly Notices of the Royal Astronomical Society, 437(2), 1352–1361. https://doi.org/10.1093/mnras/stt1964

Frierson, D. M. W., Held, I. M., & Zurita-Gotor, P. (2006). A gray-radiation aquaplanet moist GCM. Part I: Static stability and eddy scale. Journal of the Atmospheric Sciences, 63(10), 2548–2566. https://doi.org/10.1175/jas3753.1

Frierson, D. M. W., Held, I. M., & Zurita-Gotor, P. (2007). A gray-radiation aquaplanet moist GCM. Part II: Energy transports in altered climates. Journal of the Atmospheric Sciences, 64(5), 1680–1693. https://doi.org/10.1175/jas3913.1

Frierson, D. M. W., Lu, J., & Chen, G. (2007). Width of the Hadley cell in simple and comprehensive general circulation models. Geophysical Research Letters, 34(18), L18804. https://doi.org/10.1029/2007gl031115

Hartmann, D. L. (2016). Global Physical Climatology (2nd ed.). Elsevier.

Held, I. M., & Hou, A. Y. (1980). Nonlinear axially symmetric circulations in a nearly inviscid atmosphere. Journal of the Atmospheric Sciences, 37(3), 515–533. https://doi.org/10.1175/1520-0469(1980)037<0515:nascia>2.0.co;2

Hersbach, H., Bell, B., Berrisford, P., Hirahara, S., Horányi, A., Muñoz-Sabater, J., Nicolas, J., Peubey, C., Radu, R., Schepers, D., Simmons, A., Soci, C., Abdalla, S., Abellan, X., Balsamo, G., Bechtold, P., Biavati, G., Bidlot, J., Bonavita, M., … Thépaut, J.-N. (2020). The ERA5 global reanalysis. Quarterly Journal of the Royal Meteorological Society, 146(730), 1999–2049. https://doi.org/10.1002/qj.3803

Holton, J. R., & Hakim, G. J. (2013). An Introduction to Dynamic Meteorology (5th ed.). Academic Press.

Hwang, Y.-T., & Frierson, D. M. W. (2010). Increasing atmospheric poleward energy transport with global warming. Geophysical Research Letters, 37(24), L24807. https://doi.org/10.1029/2010gl045440

Jennings, K. S., Winchell, T. S., Livneh, B., & Molotch, N. P. (2018). Spatial variation of the rain–snow temperature threshold across the Northern Hemisphere. Nature Communications, 9, 1148. https://doi.org/10.1038/s41467-018-03629-7

Joshi, M. M., & Haberle, R. M. (2012). Suppression of the water ice and snow albedo feedback on planets orbiting red dwarf stars and the subsequent widening of the habitable zone. Astrobiology, 12(1), 3–8. https://doi.org/10.1089/ast.2011.0668

Laurindo, L. C., Mariano, A. J., & Lumpkin, R. (2017). An improved near-surface velocity climatology for the global ocean from drifter observations. Deep-Sea Research Part I: Oceanographic Research Papers, 124, 73–92. https://doi.org/10.1016/j.dsr.2017.04.009

Lindzen, R. S., & Hou, A. Y. (1988). Hadley circulations for zonally averaged heating centered off the equator. Journal of the Atmospheric Sciences, 45(17), 2416–2427. https://doi.org/10.1175/1520-0469(1988)045<2416:hcfzah>2.0.co;2

Lindzen, R. S., & Nigam, S. (1987). On the role of sea surface temperature gradients in forcing low-level winds and convergence in the tropics. Journal of the Atmospheric Sciences, 44(17), 2418–2436. https://doi.org/10.1175/1520-0469(1987)044<2418:otross>2.0.co;2

Manabe, S. (1969). Climate and the ocean circulation: I. The atmospheric circulation and the hydrology of the earth's surface. Monthly Weather Review, 97(11), 739–774. https://doi.org/10.1175/1520-0493(1969)097<0739:catoc>2.3.co;2

Merlis, T. M., & Schneider, T. (2010). Atmospheric dynamics of Earth-like tidally locked aquaplanets. Journal of Advances in Modeling Earth Systems, 2(4), 13. https://doi.org/10.3894/james.2010.2.13

Murray, C. D., & Dermott, S. F. (1999). Solar System Dynamics. Cambridge University Press. https://doi.org/10.1017/cbo9781139174817

Myhre, G., Highwood, E. J., Shine, K. P., & Stordal, F. (1998). New estimates of radiative forcing due to well mixed greenhouse gases. Geophysical Research Letters, 25(14), 2715–2718. https://doi.org/10.1029/98gl01908

NOAA National Centers for Environmental Information. (2022). ETOPO 2022 15 Arc-Second Global Relief Model. NOAA National Centers for Environmental Information. https://doi.org/10.25921/fd45-gt74

North, G. R. (1975). Theory of energy-balance climate models. Journal of the Atmospheric Sciences, 32(11), 2033–2043. https://doi.org/10.1175/1520-0469(1975)032<2033:toebcm>2.0.co;2

North, G. R., Cahalan, R. F., & Coakley, J. A. (1981). Energy balance climate models. Reviews of Geophysics and Space Physics, 19(1), 91–121. https://doi.org/10.1029/rg019i001p00091

Peel, M. C., Finlayson, B. L., & McMahon, T. A. (2007). Updated world map of the Köppen-Geiger climate classification. Hydrology and Earth System Sciences, 11(5), 1633–1644. https://doi.org/10.5194/hess-11-1633-2007

Pierrehumbert, R. T. (2010). Principles of Planetary Climate. Cambridge University Press. https://doi.org/10.1017/cbo9780511780783

Pierrehumbert, R. T. (2011). A palette of climates for Gliese 581g. The Astrophysical Journal Letters, 726(1), L8. https://doi.org/10.1088/2041-8205/726/1/l8

Roe, G. H. (2005). Orographic precipitation. Annual Review of Earth and Planetary Sciences, 33, 645–671. https://doi.org/10.1146/annurev.earth.33.092203.122541

Sellers, W. D. (1969). A global climatic model based on the energy balance of the earth-atmosphere system. Journal of Applied Meteorology, 8(3), 392–400. https://doi.org/10.1175/1520-0450(1969)008<0392:agcmbo>2.0.co;2

Semtner, A. J. (1976). A model for the thermodynamic growth of sea ice in numerical investigations of climate. Journal of Physical Oceanography, 6(3), 379–389. https://doi.org/10.1175/1520-0485(1976)006<0379:amfttg>2.0.co;2

Shields, A. L., Meadows, V. S., Bitz, C. M., Pierrehumbert, R. T., Joshi, M. M., & Robinson, T. D. (2013). The effect of host star spectral energy distribution and ice-albedo feedback on the climate of extrasolar planets. Astrobiology, 13(8), 715–739. https://doi.org/10.1089/ast.2012.0961

Showman, A. P., Wordsworth, R. D., Merlis, T. M., & Kaspi, Y. (2013). Atmospheric circulation of terrestrial exoplanets. In S. J. Mackwell, A. A. Simon-Miller, J. W. Harder, & M. A. Bullock (Eds.), Comparative Climatology of Terrestrial Planets (pp. 277–326). University of Arizona Press. https://doi.org/10.2458/azu_uapress_9780816530595-ch12

Slingo, J. M. (1987). The development and verification of a cloud prediction scheme for the ECMWF model. Quarterly Journal of the Royal Meteorological Society, 113(477), 899–927. https://doi.org/10.1002/qj.49711347710

Smith, R. B., & Barstad, I. (2004). A linear theory of orographic precipitation. Journal of the Atmospheric Sciences, 61(12), 1377–1391. https://doi.org/10.1175/1520-0469(2004)061<1377:altoop>2.0.co;2

Spiegel, D. S., Menou, K., & Scharf, C. A. (2008). Habitable climates. The Astrophysical Journal, 681(2), 1609–1623. https://doi.org/10.1086/588089

Stommel, H. (1948). The westward intensification of wind-driven ocean currents. Transactions, American Geophysical Union, 29(2), 202–206. https://doi.org/10.1029/tr029i002p00202

Sverdrup, H. U. (1947). Wind-driven currents in a baroclinic ocean; with application to the equatorial currents of the eastern Pacific. Proceedings of the National Academy of Sciences, 33(11), 318–326. https://doi.org/10.1073/pnas.33.11.318

Vallis, G. K. (2017). Atmospheric and Oceanic Fluid Dynamics (2nd ed.). Cambridge University Press. https://doi.org/10.1017/9781107588417

Williams, D. M., & Kasting, J. F. (1997). Habitable planets with high obliquities. Icarus, 129(1), 254–267. https://doi.org/10.1006/icar.1997.5759

Yang, J., Cowan, N. B., & Abbot, D. S. (2013). Stabilizing cloud feedback dramatically expands the habitable zone of tidally locked planets. The Astrophysical Journal Letters, 771(2), L45. https://doi.org/10.1088/2041-8205/771/2/l45
