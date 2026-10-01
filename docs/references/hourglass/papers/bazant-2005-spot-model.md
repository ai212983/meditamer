# PDF text extraction: The Spot Model for random-packing dynamics

- Author: Martin Z. Bazant
- Published version DOI: <https://doi.org/10.1016/j.mechmat.2005.06.016>
- Open source: <https://arxiv.org/abs/cond-mat/0501130>
- Local original: [PDF](bazant-2005-spot-model.pdf)
- PDF SHA-256: `faeef0da366902a9a713d4c4bd6863fd911e267d2e82cfe8ad5846a0beeb8a37`
- Extraction: Poppler `pdftotext -nopgbrk -enc UTF-8`

This generated extraction is provided for repository search and machine reading. The PDF remains
authoritative for figures, equations, columns, and pagination; extraction errors are preserved rather
than silently interpreted.

---

arXiv:cond-mat/0501130v1 [cond-mat.stat-mech] 7 Jan 2005

The Spot Model for random-packing dynamics
Martin Z. Bazant a
a

Department of Mathematics, Massachusetts Institute of Technology, Cambridge 02139

The diffusion and flow of amorphous materials, such as glasses and granular materials, has resisted a simple
microscopic description, analogous to defect theories for crystals. Early models were based on either gas-like
inelastic collisions or crystal-like vacancy diffusion, but here we propose a cooperative mechanism for dense
random-packing dynamics, based on diffusing “spots” of interstitial free volume. Simulations with the Spot
Model can efficiently generate realistic flowing packings, and yet the model is simple enough for mathematical
analysis. Starting from a non-local stochastic differential equation, we derive continuum equations for tracer
diffusion, given the dynamics of free volume (spots). Throughout the paper, we apply the model to granular
drainage in a silo, and we also briefly discuss glassy relaxation. We conclude by discussing the prospects of
spot-based multiscale modeling and simulation of amorphous materials.

over which the trajectories of neighboring particles are strongly correlated – the so-called cage
effect (Hansen and McDonald, 1986).
This difficulty is extended to much larger
length and time scales in glassy relaxation (Angell et al., 2000) and dense granular
flow (Jaeger et al., 1996), where kinetic energy is
insufficient to easily tear a particle away from its
cage of neighbors. As a result, one must somehow
describe the cooperative motion of all particles
at once. In dense ordered materials (crystals),
cooperative relaxation and plastic flow are mediated by defects, such as interstitials, vacancies,
and dislocations, but it is not clear how to define
“defects” for homogeneous disordered materials.
The challenge in describing random-packing
dynamics is related to the concept of “hyperuniform” point distributions, recently introduced
by Torquato and Stillinger (2003). In a dilute
gas, particles undergo independent random walks
and thus have the “uniform” distribution of a
Poisson process (Hadjiconstantinou et al., 2003),
where the variance of the number of particles, N ,
scales with the volume, V : Var(N ) = hN i = ρV ,
where ρ is the mean density. In a condensed
phase, the particle distribution must be “hyperuniform”, with much smaller fluctuations, proportional to the surface area: Var(N ) ∝ V (d−1)/d ,
where d = 3 is the spatial dimension, so it is clear
that particles cannot fluctuate independently. In

Dedicated to Prager Medalist, Salvatore Torquato.
1. Introduction
Professor Torquato has made pioneering contributions to the characterization of random
packings and their relation to properties of heterogeneous materials (Torquato, 2003). His recent work rejects the classical notion of “random
close packing” of hard spheres and replaces it
with the more precise concept of a “maximally
random jammed state” (Torquato et al., 2000;
Torquato and Stillinger, 2001; Kansal et al.,
2002; Donev et al., 2004b). In these studies and
others (O’Hern et al., 2002, 2003), however, the
focus is on the statistical geometry of static packings, and not on the dynamics of nearly jammed
packings in flowing amorphous materials.
Dense random-packing dynamics is at the heart
of condensed matter physics, and yet it remains
not fully understood at the microscopic level.
This is in contrast to dilute random systems
(gases), where Boltzmann’s kinetic theory provides a successful statistical description, based on
the hypothesis of randomizing collisions for individual particles. The same single-particle theory
can also be applied to molecular liquids at typical experimental time and length scales, where
kinetic energy is able to fully disrupt local packings. The difficulty arises in describing liquids
at very small (atomic) length and time scales,
1

2

Figure 1. Eyring’s mechanism for flow in viscous
liquids: A single particle jumps from one available
“cage” to another by exchanging with a “void”
moving in the opposite direction.

a crystal, hyper-uniformity is a property of the
ideal lattice, which is preserved during diffusion
and plastic flow by the motion of isolated defects.
For dense disordered materials, however, no simple flow mechanism has been identified, which
preserves hyper-uniformity.
Eyring (1936) was perhaps the first to suggest a microscopic mechanism for viscous flow in
liquids, analogous to vacancy diffusion in crystals. He proposed that the packing evolves when
individual particles jump into available cages,
thus displacing pre-existing voids, as shown in
Fig. 1. Much later, the same hypothesis was put
forth independently in theories of the glass transition (Cohen and Turnbull, 1959), shear flow
in metallic glasses (Spaepen, 1977), granular
drainage from a silo (Litwiniszyn, 1958; Mullins,
1972), and compaction in vibrated granular materials (Boutreux et al., 1998), although it is not
clear that all of these authors intended for the
model to be taken literally at the microscopic
level. Since particles and voids simply switch
places, it seems the Void Model can only be simulated on a single, fixed configuration of particles,
but real flows are clearly not constrained in this
way. This difficulty is apparent in the work of
Caram and Hong (1991), who neglected random
packings and simulated the Void Model on a lat-

M. Z. Bazant
tice in an attempt to describe granular drainage.
By now, free volume theories of amorphous materials (based on voids) have fallen
from favor, and experiments on glassy relaxation (Weeks et al., 2000) and granular
drainage (Choi et al., 2004) have demonstrated
that packing rearrangements are highly cooperative and not due to single-particle hops. In a
recent theory of granular chute flows down inclined planes, Ertaş and Halsey (2002) have postulated the existence of coherent rotations, called
“granular eddies” to motivate a continuum theory
of the mean flow. Although the theory successfully predicts Bagnold rheology and the critical
layer thickness for flow, Landry and Grest (2003)
have failed to find any evidence for granular eddies in discrete-element simulations of chute flow
(Landry et al., 2003).
In glass theory, Adam and Gibbs (1965) introduced the concept of regions of cooperative relaxation, whose sizes diverge at the glass
transition. Modern statistical mechanical approaches are based on mode-coupling theory
(Götze and Sjögren, 1992), which accurately predicts density correlation functions in simple liquids (Kob, 1997), although a clear microscopic
mechanism, which could be used in a particle
simulation, has not really emerged. Cooperative rearrangements have also long been recognized in the literature on sheared glasses. Orowan
(1952) was perhaps the first to postulate localized shear transformations in regions of enhanced atomic disorder. Argon (1979) later developed the idea of “intense shear transformations” at low temperature, which underlies the
stochastic model of “localized inelastic transformations” (Bulatov and Argon, 1994). A similar
notion of “shear transformation zones” (STZ) has
also been invoked by Falk and Langer (1998) in
a continuum theory of shear response, which has
recently been extended to account for free-volume
creation and annihilation in glasses (Lemaître,
2002b) and granular materials (Lemaître, 2002a).
This phenomenology seems to capture many universal features of amorphous materials, although
the microscopic picture of “+” and “−” STZ
states remains vague.
In this paper, we propose a simple model for

The Spot Model for random-packings dynamics

3

the kinematics of dense random packings. In
section 2, we introduce a general mechanism for
structural rearrangements based on the concept
of a diffusing “spot” of free volume. In section 3,
we apply the Spot Model to granular drainage.
In section 4, we analyze the diffusion of a tracer
particle via a non-local, nonlinear stochastic differential equation, in the limit of an ideal gas of
spots. In section 5, we derive equations for tracer
diffusion in granular drainage, which depend on
the density, drift, and diffusivity of spots (or free
volume). We close by discussing possible applications to glasses in section 6 and spot-based multiscale modeling and simulation of amorphous materials in section 7.

simply increases the injection rate of voids, but
not their geometrical trajectories. However, the
model incorrectly predicts cage breaking and mixing at the scale of individual particles. This
“paradox of granular diffusion” is a fruitful starting point for a new model of random-packing dynamics (Bazant et al., 2005).

2. The Spot Model
2.1. Motivation
Our intuition tells us that a particle in a dense
random packing must move together with its
nearest neighbors over short distances, followed
by gradual cage breaking at longer distances. In
simple liquids, this transition occurs at the molecular scale (< nm) over very short times (< ps)
compared to typical experimental scales. In supercooled liquids and glasses, the time scale for
structural relaxation effectively diverges and is replaced by slow, power-law decay (Angell et al.,
2000; Kob, 1997; Hansen and McDonald, 1986),
although the length scale for cooperative motion
remains relatively small (Weeks et al., 2000). In
granular drainage, cage breaking occurs slowly,
over time scales comparable to the exit time from
the silo, so that cooperative motion is important
throughout the system at the macroscopic scale
(Choi et al., 2004).
Another curious feature of granular drainage is
the importance of geometry: all fluctuations in a
dense flow seem to have a universal dependence
on the distance dropped for a wide range of flow
rates (Choi et al., 2004). In a sense, therefore, increasing the flow speed in this regime is like fastforwarding a film, passing through the same sequence of configurations, only more quickly. The
only existing theory to predict this property (as
well as the mean flow profile in silo drainage) is
the Void Model, since increasing the flow speed

2.2. General formulation
Let us suppose that the cage effect gives rise
to spatial correlations in particle velocities, with
correlation coefficient, C(r), for two particles separated by r. More generally, when there is broken symmetry, e.g. due to gravity in granular drainage, there is a correlation coefficient,
Cpαβ (r1 , r2 ), for the α velocity component of a
particle at r1 and the β velocity component of a
particle at r2 . Perhaps the simplest way to encode this information in a microscopic model is to
imagine that particles move cooperatively in response to some extended entity – a spot – which
causes a particle at rp to move by
∆Rp = −w(rp , rs ) ∆Rs

(1)

when it moves by ∆Rs near rs . Although the
spot is not a “defect” per se, like a dislocation
in an ordered packing, it is a collective excitation
which allows a random packing to rearrange.
In principle, the spot influence, w, could be
a matrix causing a smooth distribution of local
translation and rotation about the spot center rs ,
but a reasonable first approximation is that it is
simply a collective translation the opposite direction from the spot displacement, as illustrated in
Fig. 2. In this case, w is a scalar function, whose
shape is roughly that of the velocity correlation
function. More precisely, under some simple assumptions, we show below that C(r) is the overlap integral of two spot influence functions separated by r. Since the spot influence is related to
the cage effect, we expect that w and C will decay
quickly with distance, for separations larger than
a few particle diameters. Due to the local statistical regularity of dense random packings, we might
expect a spot to retain its “shape” as it moves, in
which case w depend only on the separation vec(i)
(j)
tor, rp − rs , although this assumption might

4

Figure 2. The spot mechanism for cooperative
diffusion: A group of neighboring particles makes
small correlated displacements in response to a
diffusing “spot” of excess interstitial volume.

M. Z. Bazant

Figure 3. The trails of spots correspond to transient, reptating chains of particles.

(j)

rs induces a random displacement, ∆R(i)
p , of the
(i)

need to be relaxed in regions of large gradients in
density or velocity.
Physically, what is a spot? Since particles move
collectively in one direction, a spot must correspond to some amount of free interstitial volume
(or “missing particles”) moving in the other direction. If particles are distributed with number
density ρp (rp ) and a spot at rs carries a typical
volume, Vs (rs ), then an approximate statement
of volume conservation is
Z
Vs ∆Rs = − drp ρ(rp )w(rp , rs )∆Rp (rp ).
(2)
For particles distributed uniformly at volume
fraction, φ, this reduces to a simple expression
for the local volume carried by a spot,
Z
Vs (rs ) = φ drp w(rp , rs ),
(3)
which can thus be interpreted as a measure of the
spot’s “total influence”.
A very simple, spot-based Monte Carlo simulation proceeds as follows. Given a distribution of
(passive) particles and (active) spots, the random
displacement, ∆Rs(j) , of the jth spot centered at

ith particle centered at rp :
(j)
(j)
(i) (j)
∆R(i)
p = −w(rp , rs + ∆Rs ) ∆Rs

(4)

Each spot undergoes an independent random
walk, with an appropriate drift and diffusivity for
free volume, which leaves in its trail a thick chain
of particles reptating in the opposite direction, as
shown in Fig. 3. In Eq. (4), we choose to center
the spot influence on the end of its small displacement, but it is also reasonable to use the midpoint
of the displacement(Rycroft et al., 2005). In the
infinitesimal limit (see below), these choices are
analogous to different definitions of stochastic differentials (Risken, 1996).
In principle, the drift velocity, diffusivity, and
influence function of spots could depend on local variables, such as stress, and temperature
(or rather, some suitable microscopic quantities
related to contact forces and velocities, respectively). Spots could also interact with each other,
undergo creation and annihilation, and possess a
statistical distribution of sizes (or influence functions). The simplest kinematic assumption, however, which captures the basic physics of the
cage effect, is that spots are identical and maintain their properties while undergoing independent (non-interacting) random walks. In particu-

5

The Spot Model for random-packings dynamics
lar, the constant influence function, w(|rp − rs |),
is chosen to be translationally invariant in space
and time. It turns out that this model allows
rather realistic multiscale simulations, while remaining analytically tractable.

2.3. Multiscale simulation
The simple spot mechanism above gives a reasonable description of tracer diffusion and slow
cage breaking in random packings, but it does
not strictly enforce packing constraints (or, more
generally, inter-particle forces). As such, particles perform independent random walks in the
long-time limit, which eventually leads to uniform
density fluctuations with Poisson statistics. For
a complete microscopic model, we must somehow
preserve hyper-uniform packings.
This may be accomplished by adding a relaxation step to the spot-induced displacements, as
shown in Fig. 4. First in (a), a spot displacement
causes a simple correlated displacement, as described above, e.g. using Eq. (4) with some shortranged choice of w(r) with a finite cutoff. Next
in (b), the affected particles and a shell of their
nearest neighbors are allowed to relax under appropriate inter-particle forces, with more distant
particles held fixed. For simulations of (nearly)
hard grains, the most important forces come from
a soft-core repulsion, which pushes particles apart
only if particle begin to overlap.
Although it is not obvious a priori, the net
spot-induced cooperative displacements, shown
in Fig. 4(c), easily produce very realistic flowing packings, while preserving the physical picture of the model (Rycroft et al., 2005). In practice, the correlated nature and small size of the
spot-induced block displacements results in very
small and infrequent particle overlaps, only near
the edges of the spot, where some shear occurs
with the background packing. As a result, it
seems the details of the relaxation are not very
important, although this issue merits further investigation. In any case, the algorithm is interesting in its own right as a method of multiscale
modeling, since it combines a macroscopic simulation of simple extended objects (spots) with
localized, microscopic simulations of particles.

3. Application to Granular Drainage
3.1. Spot parameters
The classical Kinematic Model for the mean
velocity in granular drainage (Nedderman, 1991),
which compares fairly well with experiments
(Tüzün and Nedderman, 1979; Samadani et al.,
1999; Choi et al., 2005), postulates that the mean
downward velocity, v, satisfies a linear diffusion
equation,
∂v
= b ∇2⊥ v
∂z

(5)

where the vertical distance z plays the role of
“time” and the horizontal dimensions (with gradient, ∇⊥ ) play the role of “space”. The microscopic justification for Eq. (5) is the Void
Model of Litwiniszyn (1958, 1963) and Mullins
(1972, 1974), where particle-sized voids perform
directed random walks upward from the orifice.
As discussed above, this microscopic mechanism
is firmly contradicted by particle-tracking experiments (Choi et al., 2004), but, as shown below, a
similar macroscopic flow equation can be derived
from the Spot Model, where spot diffuse upward
with a (horizontal) diffusion length,
b=

Var(∆xs )
2dh ∆zs

(6)

where ∆xs is the random horizontal displacement
of a spot as it rises by ∆zs and dh = 2 is the
horizontal dimension. A typical value for 3 mm
glass beads is b ≈ 1.3d, where d is the particle
diameter.
The shape of the spot influence function can
be inferred from measurements of spatial velocity
correlations in experiments (Bazant et al., 2005)
or simulations by the discrete-element method
(DEM) (Rycroft et al., 2005). The simplest assumption is a uniform spherical influence with a
finite cutoff,

w r < ds /2
w(r) =
(7)
0 r > ds /2
where experiments and simulations find ds ≈ 5d.
This is consistent with our interpretation of the
spot mechanism in terms of the cage effect, where

6

M. Z. Bazant

(a)

(b)

(c)

Figure 4. Multiscale simulation of densely packed (nearly) hard spheres with the Spot Model: (a) A
block of neighboring particles translates opposite to the displacement of a spot of free volume; (b) the
block and a shell of neighbors are allowed to relax under soft-core repulsive forces; (c) the net cooperative
rearrangement combines these two steps. (Particle displacements are greatly exaggerated for clarity.)

a particle moves with its nearest neighbors. The
typical number of particles affected by a spot,
 3
ds
N =φ
,
(8)
d
is thus N ≈ 72, for φ ≈ 0.58.
For a uniform spot, the condition of volume
conservation, Eq. (2), reads
Vs (∆xs , ∆zp ) = −N Vp (∆xp , ∆zp ).

(9)

Using Eq. (1), this provides an expression for the
spot influence,
w=

Vs
∆φ
≈ 2
N Vp
φ

(10)

in terms of ∆φ, the change in local volume fraction due to the presence of a single spot. In
discrete-element simulations of granular spheres
in silo drainage (Rycroft et al., 2005), the local
volume fraction varies in the range, φ = 0.565 −
0.605, within the rough bounds of jamming, φ =
0.63 (Torquato et al., 2000; Kansal et al., 2002;
O’Hern et al., 2002, 2003), and random loose
packing, φ = 0.55 (Onoda and Liniger, 1990).
If we attribute ∆φ/φ = 1% to a single spot,
then we find w ≈ 0.017, but, if many spots, say

Ns = 10, can overlap, then this estimate is reduced by 1/Ns . We thus expect, w = 10−3 −10−2,
which can be tested against experiments.
We can also use Eqs. (9)-(10) to infer w from a
measurement of the horizontal particle diffusion
length:
bp =

Var(∆xp )
w2 Var(∆xs )
= wb
=
2dh |∆zp |
2dh w∆zs

(11)

Discrete-element simulations (Rycroft et al.,
2005)
and
particle-tracking
experiments
(Choi et al., 2004) for similar flows yield
w = bp /b = 0.00286d/1.14d = 0.00250 and
w = d/bPe x = d/(1.3d)(321) = 0.0024, respectively (where Pe x is a Péclet number). These
values are consistent with the predictions of the
model.
A spot’s influence is related to the free volume
it carries by Eq. (3). In the case of a uniform spot
of diameter, ds , its total free volume is given by
Vs =

πφwd3s
6

(12)

which is related to the particle volume, Vp , by
Vs
= wφ
Vp



ds
d

3

= wN.

(13)

The Spot Model for random-packings dynamics

7

For dense granular drainage, the typical values
N = 72 and w = 0.0025 imply Vs = 0.18Vp , so
a spot carries only around one fifth of a particle volume, spread out over a region of roughly
five particle diameters. This delocalized picture
of free volume diffusion is radically different from
the classical Void Model, which is the (unphysical) limit where Vs = Vp and N = w = 1.

Next, we consider the multiscale spot algorithm
in Fig. 4, with an internal relaxation step, applied
to a three-dimensional drainage simulation, starting from the same initial condition and geometry
as the DEM simulation in Fig. 5(d). The simplest possible relaxation scheme is to push any
pair of overlapping particles in a relaxation zone
(a sphere of diameter, ds +4d) apart by a displacement, α(d − r) proportional to the overlap, d − r,
while keeping particles fixed outside a sphere of
diameter, ds + 2d. A spot simulation with similar
parameters as above, including such a relaxation
step with α = 0.8, is shown in Fig. 5(c). The
rate of introducing spots at the orifice and their
upward drift velocity has also been calibrated for
comparison to the DEM simulation, at the same
instant in time.
Clearly, the simple multiscale relaxation step
is able to preserve realistic random packings, and
in many ways the spot simulation in Fig. 5(c)
is indistinguishable from the much more computationally demanding simulation in Fig. 5(d).
Not only are the mean velocity profile and diffusion length reproduced, but so are various microscopic statistics of the packing geometry, such
as the two-body and three-body correlation functions (Rycroft et al., 2005). These surprising results are quite insensitive to the details of the
relaxation step, apparently due to the very small
and infrequent particle overlaps which arise from
the cooperative mechanism in the spot simulation. Another, deeper reason may be that the
geometry of dense flowing random packings has
universal features, which are achieved by the totally different dynamics of spot and DEM simulations.

3.2. Spot simulations
As described above, the Spot Model can be successfully calibrated for granular drainage to enable very simple and efficient simulations. First,
we consider the basic mechanism in Fig. 2 and
Eq. (4) applied to granular drainage in a quasitwo-dimensional silo with a narrow opening, far
from the side walls, as in the experiments of
Samadani et al. (1999) and Choi et al. (2004).
For simplicity, we simulate the model in two dimensions (dh = 1) using uniform spots with the
parameters determined from experiments above
(w = 0.0024, ds = 5d, b = 1.3d). The simulation
begins with a random packing of identical disks,
colored with horizontal stripes (10d thick) to aid
in visualizing the subsequent evolution.
A snapshot of the spot simulation at a later
time is shown in Fig. 5(a). For comparison, a
simulation of the same situation with the Void
Model on a two-dimensional lattice, following
Caram and Hong (1991), is shown in Fig. 5(b),
along with the central slice of a three-dimensional
DEM simulation (15d thick) in Fig. 5(d), which
is very similar to the experiment. Although the
mean flow profile is similar in the spot and void
simulations and reasonably close to experiment,
the void simulation displays far too much diffusion and cage breaking, since the initial horizontal
stripes are completely mixed down to the single
particle level inside the flow region. In contrast,
the interfaces between the colored layers remain
fairly sharp in the spot simulation, as in experiments and the DEM simulations. Tracer diffusion
is described fairly well by the spot simulation, although the lack of packing constraints eventually
leads to the loss of hyper-uniformity, as particles
begin to overlap and open gaps in the lower side
regions of highest shear. For more details, see
Bazant et al. (2005).

4. Mathematical analysis of diffusion
4.1. A stochastic differential equation
In this section, we return to the general formulation of the Spot Model and analyze tracer diffusion in the continuum limit. It is clear from the
simulations in Fig. 5 that the basic model in Fig. 2
gives a reasonable description of the dynamics of
a single particle tracer, even though the multiscale relaxation step in Fig. 4 is needed to preserve realistic packings. The relatively small size

8

M. Z. Bazant
80

80

70

70

60

60

50

50

40

40

30

30

20

20

10

10

(a)

(c)

0

−20

−10

0

10

20

(b)

0

−20

−10

0

10

20

(d)

Figure 5. Simulations of granular drainage in a quasi-two-dimensional silo. Top: Two-dimensional
simulations using (a) the Spot Model without packing constraints and (b) the Void Model (Bazant et al.,
2005). Bottom: Three-dimensional simulations using (c) the Spot Model with multiscale relaxation and
(d) the Discrete Element Method for frictional, visco-elastic spheres (Rycroft et al., 2005). Particles are
colored according to their initial positions in horizontal stripes, 10d thick.

9

The Spot Model for random-packings dynamics
of the relaxation displacements makes it reasonable to regard them a small additional “noise” in
a mathematical analysis of tracer diffusion. Here,
we will neglect this small (but complicated) noise
and view its average effect as incorporated statistically into the spot influence function, w(rp , rs ),
in Eq. (4).
We begin by partitioning space as shown Fig. 6,
(n)
where the nth volume element, ∆Vs , centered
(n)
(n)
at rs contains a random number, ∆Ns , of
spots at time t (typically one or zero). In a time
interval, ∆t, suppose that the jth spot in the
nth volume element makes a random displacement, ∆R(j,n)
(which could be zero). According
s
to Eq. (4), the total displacement, ∆Rp , of a particle at rp in time ∆t is then given by a sum of
all the random displacements induced by nearby
moving spots,

d R s (rs’)
d R p(rp)
d R s (rs )
dNs= 0

Figure 6. Sketch of a particle interacting with
a collection of passing spots, showing some of
the quantities involved in the nonlocal SDE, Eq.
(15).

(n)

∆Rp = −

s
X
X ∆N

n

dNs= 1

(j,n)
.
)∆R(j,n)
w(rp , r(n)
s
s + ∆Rs

j=1

(14)
Note that the spatio-temporal distribution of
(n)
spots, ∆Ns , is another source of randomness,
in addition to the individual spot displacements,
∆R(j,n)
, so that each particle displacement is
s
given by a random sum of random variables.
In the limit of infinitesimal displacements, we
arrive at a non-local, nonlinear stochastic differential equation (SDE):
Z
dRp (t) = − dNs (rs , t)
(15)
w(rp (t), rs + dRs (rs , t)) dRs (rs , t),
where the stochastic integral is defined by the
usual limit (infinitely refined partition of space)
of the random Riemann sum in Eq. (14). This
equation differs from standard nonlinear SDEs
(Risken, 1996) in two basic ways: (i) The tracer
trajectory,
Z t
rp (t) =
dRp (τ )
(16)
τ =0

is passively driven by a stochastic distribution
of moving influences (spots), dNs (rs , t), which
evolves in time and space, rather than by some

internal source of independent noise, and (ii) the
stochastic differential, dRp (t), is given by a nonlocal integral over other stochastic differentials,
dRs (rs , t), associated with these moving influences, which lie at positions, rs , at finite distances
away from the particle at rp .
4.2. A Fokker-Planck equation
In general, the various stochastic differentials
in Equation (15) are correlated, which significantly complicates analysis. Here, we will make
the reasonable first approximation of an ideal gas
of spots, where the tracer particle sees an independent, random configuration of non-interacting
spots at each infinitesimal time step. As in an
ideal gas (Hadjiconstantinou et al., 2003), spots
are thus distributed according to a Poisson process with a given mean density, ρs (rs , t). In addition to neglecting correlations caused by interactions between spots, we disregard the following facts: (i) The distribution of spots in space,
{dNs (rs , t)}, at time t depends explicitly on the
distribution and displacements at the previous
time, t − dt, via the spot random walks; (ii) Each

10

M. Z. Bazant

spot, due to its finite range of influence, affects
the same particle for a finite period of time, so
any persistence (autocorrelation) in the spot trajectory is transferred to the particles, in a nonlinear fashion controlled by w(rp , rs ).
In the spot-gas approximation, the tracer particle performs a random walk with independent
(but non-identically distributed) displacements,
which depend non-locally on a Poisson process
for finding spots. Therefore, the propagator,
Pp (r, t|r0 , t0 ), which gives the probability density
of finding the particle at r at time t after being at
r0 at time t0 , satisfies the following Fokker-Planck
equation (Risken, 1996),
∂Pp
+ ∇ · (up Pp ) = ∇∇ : (Dp Pp ),
∂t
with drift velocity,
up (r, t) =

(17)

hdRp (r, t)i
h∆Rp (r, t)i
= lim
, (18)
∆t→0
dt
∆t

and diffusivity tensor,
Dαβ
p (r, t) =

hdRpα dRpβ i
.
2 dt

(19)

P P ∂ 2 Aαβ
(Here ∇∇ : A denotes
α
β ∂xα ∂xβ .) The
Fokker-Planck coefficients may be calculated
by taking the appropriate expectations using
(n)
Eq. (14) in the limits ∆Vs → 0 and ∆t → 0
(in that order), which is straightforward since we
assume that spots do not interact. Here, the spot
(n)
displacements, ∆R(j)
s (rs ), and the local num(n)
bers of spots, ∆Ns , are independent random
variables in each time interval, and they are independent of the same variables at earlier times.
In order to calculate the drift velocity, we need
only the mean spot density, ρs (rs , t), defined by
(n)
(n)
h∆Ns i = ρs (rs , t) ∆Vs . The result,
Z
up (rp , t) = − dVs w(rp , rs ) [ρs (rs , t)us (rs , t)
−2 Ds (rs , t) · ∇ρs (rs , t)]

(20)

exhibits two sources of drift. The first term in
the integrand is a particle drift velocity, which
opposes the spot drift velocity,
hdRs (r, t)i
us (r, t) =
.
dt

(21)

as in Eq. (9). The second term, which depends
on the spot diffusion tensor,
(i)

(j)

hdRs dRs i
,
(22)
2 dt
is a “noise-induced drift”, typical of nonlinear
SDEs (Risken, 1996), which causes particles to
climb gradients in the spot density. This extra
drift is crucial to ensure that particles eventually
move toward the source of spots, e.g. the orifice
in granular drainage. Both contributions to the
drift velocity in Eq. (20) are averaged non-locally
over a finite region, weighted by the spot influence
function, w(rp , rs ).
In order to calculate the diffusivity tensor, we
also need information about fluctuations in the
spot density. From the spot-gas approximation,
we have

Ds(i,j) (r, t) =

h∆Ns(n) ∆Ns(m) i = δm,n h(∆Ns(n) )2 i = O((∆Vs(n) )ν ),
where ν = 1 for a Poisson process and ν < 1 for a
hyper-uniform process (Torquato and Stillinger,
2003). It turns out that such fluctuations do not
contribute to the diffusion tensor (in more than
one dimension), and the result is
Z
Dp (rp , t) = dVs w(rp , rs )2 ρs (rs , t)Ds (rs , t).
(23)
Note that the influence function, w, appears
squared in Eq. (23) and linearly in Eq. (20), which
causes the Péclet number for tracer particles to
be of order w smaller than that of spots (or free
volume), as in Eq. (11).
Higher-order terms a Kramers-Moyall expansion generalizing Eq. (17) for finite independent
displacements, which do depend on fluctuations
in the spot density, are straightforward to calculate, but beyond the scope of this paper. Such
terms are usually ignored because, in spite of improving the approximation, they tend to produce
small negative probabilities in the tails of distributions (Risken, 1996). In granular materials,
however, velocity gradients can be highly localized, so the correction terms could be useful.
4.3. Spatial velocity correlation tensor
For any stochastic process representing the motion of a single particle, it is well-known that

The Spot Model for random-packings dynamics

11

transport coefficients can be expressed in terms
of temporal correlation functions via the GreenKubo relations (Risken, 1996). For example, the
diffusivity tensor in a uniform flow is given by
the time integral of the velocity auto-correlation
tensor,
Z ∞
dthUpα (t)Upβ (0)i
(24)
Dpαβ =

where we have assumed that the spot influence
function, and thus the correlation tensor, is translationally invariant (r = r1 − r2 ). In this limit,
as mentioned above, the velocity correlation function is simply given by the (normalized) overlap
integral for spot influences separated by r.

0

where Up (t) = {Upα } = dRp /dt is the stochastic
velocity of a particle. (A similar relation holds
for spots.)
In the Spot Model, nearby particles move cooperatively, so the transport properties of the collective system also depend on the two-point spatial
velocity correlation tensor,
hUpα (r1 )Upβ (r2 )i
Cpαβ (r1 , r2 ) = q
hUpα (r1 )2 ihUpβ (r2 )2 i

(25)

which is normalized so that Cpαβ (r, r) = 1. We
emphasize that the expectation above is conditional on finding two particles at r1 and r2 at a
given moment in time and includes averaging over
all possible spot distributions and displacements.
Substituting the SDE (15) into Eq. (25) yields
R
Cpαβ (r1 , r2 ) = dVs ρs (rs )w(r1 , rs )w(r2 , rs )
q
Dsαβ (rs )/ Dpαβ (r1 )Dpαβ (r2 ) (26)
assuming independent spot displacements.
Equation (26) is an integral relation for cooperative diffusion, which relates the spatial velocity
correlation tensor to the spot (or free volume) diffusivity tensor via integrals of the spot influence
function, w(rp , rs ). If the statistical dynamics of
spots is homogeneous (in particular, if Ds is constant), then the relation simplifies:
Cpαβ (r1 , r2 ) =
qR

R

dVs ρs (rs ) w(r1 ,rs ) w(r2 ,rs )

dVs ρs (rs ) w(r1 ,rs )2

R

(27)
.

dVs′ ρs (r′s ) w(r2 ,r′s )2

if also the tensor is diagonal, Dsαβ ∝ δα,β . If the
statistical dynamics of particles is also homogeneous, as in a uniform flow (ρs = constant), then
it simplifies even further:
R
dVs w(r − rs ) w(−rs )
R
(28)
Cpαβ (r) =
dVs w(rs )2

4.4. Relative diffusion of two tracers
The spatial velocity correlation function affects
many-body transport properties. For example,
the relative displacement of two tracer particles,
r = r1 − r2 , has an associated diffusivity tensor
given by,
Dαβ (r1 , r2 ) =

Dpαβ (r1 ) + Dpαβ (r2 )
(29)
q
−2 Cpαβ (r1 , r2 ) Dpαα (r1 )Dpββ (r2 )

In a uniform flow, the diagonal components take
the simple form

(30)
Dαα (r) = 2 Dpαα 1 − Cpαα (r)

which may be used above to estimate the cagebreaking time, as the expected time for two particles diffuse apart by more than one particle diameter. A more detailed calculation of the relative propagator, P (r, t|r0 , t0 ), neglecting temporal correlations (as above) would start from the
associated Fokker-Planck equation,
∂P (r, t)
= ∇∇ : (D(r)P (r, t))
(31)
∂t
with a delta-function initial condition. (In a nonuniform flow, one must also account for noiseinduced drift and motion of the the center of
mass.) This analysis does not enforce packing
constraints, so it allows for two particles to be
separated by less than one diameter. A hardsphere repulsion may be approximated by a reflecting boundary condition at |r| = d when
solving equations such as (31), but there does
not seem to be any simple way to enforce interparticle forces exactly in the analysis.
5. Tracer diffusion in granular drainage
5.1. Statistical dynamics of spots
The analysis in the previous section makes no
assumptions about spots, other than the existence of well-defined local mean density, mean velocity, and diffusion tensor, which may depend on

12
time and space. As such, the results may have relevance for a variety of dense disordered systems
exhibiting cooperative diffusion (see below). In
this section, we apply the model to the specific
case of granular drainage, in which spots diffuse
upward from a silo orifice, as in Fig. 5. Our goal
here is simply to show how to derive continuum
equations from the Spot Model in a particular
case, but not to study any solutions in detail.
For simplicity, let us assume that each spot undergoes mathematical Brownian motion with a
vertical drift velocity, us = vs ẑ, and a diagonal
diffusion tensor,

 ⊥
Ds
0
0
Ds⊥ 0 
Ds =  0
(32)
k
0
0
Ds
which allows for a different diffusivity in
the horizontal (⊥) and vertical (k) directions due to symmetry breaking by gravity.
In that case, the propagator for a single
“spot tracer”, Ps (x, z, t|x0 , z0 , t), satisfies another
Fokker-Planck equation,


∂2 
∂
∂Ps
+
(vs Ps ) = ∇2⊥ Ds⊥ Ps + 2 Dsk Ps .
∂t
∂z
∂z

M. Z. Bazant
The time-dependent mean density of spots,
ρs (x, z, t), depends on the mean spot injection
rate, Q(x0 , z0 , t) (number/area×time), which
may vary in time and space due to complicated
effects such as arching and jamming near the orifice. It is natural to assume that spots are injected at random points along the orifice (where
they fit) according to a space-time Poisson process with mean rate, Q. In that case, if spots
do not interact, the spatial distribution of spots
within the silo at time t is also a Poisson process
with mean density,
Z
Z
Z
ρs (x, z, t) =
dx0 dz0
dt0
(35)
t0 <t

Q(x0 , z0 , t) Ps (x, z, t|x0 , z0 , t0 ).
For a point-source of spots (i.e. an orifice roughly
one spot wide) at the origin with flow rate, Q0 (t)
(number/time), this reduces to
Z
dt0 Q0 (t0 ) Ps (x, z, t|0, 0, t0),
ρs (x, z, t) =
t0 <t

(36)

where Ps is the usual Gaussian propagator for
Eq. (33) in the case of constant us and Ds . In
(33) reality, spots should weakly interact, but the success of the Kinematic Model suggests that spots
The coefficients may depend on space (e.g. larger
diffuse independently as a first approximation in
velocity above the orifice than near the stagnant
granular drainage (Choi et al., 2005).
region), as suggested by the shape of some exper5.2. Statistical dynamics of particles
imental density waves (Baxter et al., 1989).
Integral formulae for the drift velocity and difThe
geometrical
spot
propagator,
fusivity tensor of a tracer particle may be obPs (x, |z, x0 , z0 ), is the conditional probability
tained by substituting the spot density which
of finding a spot at horizontal position x once it
solves Eq. (35) into the general expressions (20)
has risen to a height z from an initial position
and (23), respectively. For example, if spots only
(x0 , z0 ). For constant vs and Ds , the geometrical
k
propagator satisfies the diffusion equation,
diffuse horizontally (Ds = 0), then the mean
downward velocity of particles is given by
∂Ps
Z
= b ∇2⊥ Ps
(34)
∂z
vp (r, t) = dVs w(rp , rs ) ρs (rs , t) vs (rs , t) (37)
where b = Ds⊥ vs is the kinematic parameter. If
spots move independently, this equation is also
Note that the mean particle velocity is a nonlocal
satisfied by the steady-state mean spot density,
average of nearby spot drift velocities.
ρs (x, z), analogous to Eq. (5) of the Kinematic
For simplicity, let us consider a bulk region
Model. However, the mean particle velocity in
where the spot density varies on scales much
larger than the spot size. In this limit, the inthe Spot Model, Eq. (20), is somewhat different,
tegrals over the spot influence function reduce to
as it involves nonlocal effects (see below).

13

The Spot Model for random-packings dynamics
the following “interaction volumes”:
Z
Vk (r) = drs w(r, rs )k

6. Possible application to glasses

We have seen that the Spot Model, in its simplest form, accurately reproduces the kinematics
of bulk granular drainage, so it is tempting to
for k = 1, 2. (Note that V1 = Vs above.) The
speculate that it might be extended to flows in
equation for tracer-particle dynamics (17) then
other amorphous materials. In this section, we
takes the form,
briefly consider evidence for spot-like dynamics



in glasses, but we leave further extensions of the
∂Pp
∂ρs
∂
Spot Model for future work.
(39)
vs ρs − 2Dsk
V1 Pp
=
∂t
∂z
∂z
Experiments have revealed ample signs of “dy
−2∇⊥ · Ds⊥ (∇⊥ ρs )V1 Pp
namical heterogeneity” in supercooled liquids and

glasses (Hansen and McDonald, 1986; Kob, 1997;

∂2  k
D
ρ
V
P
+ ∇2⊥ Ds⊥ ρs V2 Pp .
s
2
p
s
Angell et al., 2000), but the direct observation of
2
∂z
cooperative motion has been achieved only reAgain, it is clear that rescaling the spot density
cently. Rather than compact regions of relaxis equivalent to rescaling time.
ation, Donati et al. (1998) have observed “stringWhen the spot dynamics is homogeneous (i.e.
like” relaxation in molecular dynamics simulaus and Ds are constants), Equation (39) simplitions of a Lennard-Jones model glass. The
fies further:
strength and length scale of correlations increases


with decreasing temperature, consistent with the
2
∂
1 ∂Pp
2
k ∂
=
+ b⊥
(ρs Pp ) (40) Adam-Gibbs hypothesis. Such cooperative mop ∇ + bp
2
vs Vs ∂t
∂z
∂z

 tion would be difficult to observe experimen∂ρs
∂
tally in a molecular glass, but Weeks et al. (2000)
Pp
−2b⊥ ∇ · (Pp ∇ρs ) − 2bk
∂z
∂z
have used confocal microscopy to reveal threedimensional clusters of faster-moving particles in
k
where b⊥ = b = Ds⊥ /vs and bk = Ds /vs are the
a dense colloids. In the supercooled liquid phase,
k
spot diffusion lengths and b⊥
=
b
V
/V
and
b
=
clusters of cooperative relaxation have widely
p
p 2
1
p
k
varying sizes, which grow as the glass transition
b V2 /V1 are the particle diffusion lengths. In this
is approached. In the glass phase, the clusters are
approximation, the latter are given by the simple
much smaller, on the order of ten particles, and
formula,
do not produce significant rearrangements on exR
k
2
b⊥
dV
w(r,
r
)
b
perimental time scales.
p
s
s
p
= k = R
(41)
⊥
These observations suggest that the Spot
b
dVs w(r, rs )
b
Model may have relevance for structural rearwhich generalizes Eq. (11) for a uniform spot with
rangements in simple glasses. String-like relaxa sharp cutoff. The physical meaning of the difation is reminiscent of the trail of a spot, in Fig. 3.
fusion lengths becomes more clear in the limit
An atomically thin chain might result from the
of uniform flow, ρs = constant. In terms of the
random walk of a spot, roughly one particle in
position in a frame moving with the mean flow,
size, but carrying less than one particle of free volζ = vp t − z, where vp = vs Vs ρs , we arrive at a
ume. Larger regions of correlated motion might
simple diffusion equation,
involve larger spots and/or collections of interact
ing spots. Some key features of the experimental

2
∂Pp
⊥ 2
k ∂
data of Weeks et al. (2000) seem to support this
(42)
= bp ∇⊥ + bp 2 Pp ,
∂ζ
∂z
idea: (i) Correlations take the form of “neighboring particles moving in parallel directions”, as
where ζ, the mean distance dropped, acts like
in Fig. 2; and (ii) the large clusters of correlated
time, consistent with the experimental findings
motion tend to be fractals of dimension two, as
of Choi et al. (2004).
(38)

14
would be expected for the random-walk trail of a
spot, as in Fig. 2(b). For a complete theory of the
glass transition, however, one would presumably
have to consider interactions between spots and
thermal activation of their creation, motion, and
annihilation.
7. Conclusion
In this paper, we have introduced a mechanism for structural rearrangements of dense random packings, due to diffusing spots of free volume. Even without inter-particle forces, the Spot
Model gives a reasonable description of tracer dynamics, which is trivial to simulate and amenable
to mathematical analysis, starting from a nonlocal stochastic differential equation. With a simple
relaxation step to enforce packing constraints, the
Spot Model can efficiently produce very realistic
flowing packings, as demonstrated by the case of
granular drainage from a silo. The spot mechanism may also have relevance for glassy relaxation
and other phenomena in amorphous materials.
Regardless of various material-specific applications, the ability to easily produce threedimensional dense random packings is interesting in and of itself. Current state-of-the-art
algorithms to generate dense random packings
are artificial and computationally expensive, especially near jamming (Torquato et al., 2000;
Kansal et al., 2002; O’Hern et al., 2002, 2003).
A popular example is the molecular dynamics
algorithm of Lubachevsky and Stillinger (1990),
which simulates a dilute system of interacting
particles, whose size grows linearly in time until
jamming occurs. For each random packing generated, however, a separate molecular dynamics
simulation must be performed. In contrast, the
Spot Model produces a multitude of dense random packings (albeit with some correlations between samples) from a single simulation, which is
more efficient than molecular dynamics, since it
does not require the mechanical relaxation of all
particles at once. It would be interesting to characterize the types of dense packings generated by
the Spot Model and compare with the results of
other algorithms.
The Spot Model (with relaxation) also provides

M. Z. Bazant
a convenient paradigm for multiscale modeling
and simulation of amorphous materials, analogous to defect-based modeling of crystals. The
iteration between global “mesoscopic” simulation
of spots and local “microscopic” simulation of
particles leads to a tremendous savings in computational effort, as long as the spot dynamics is
physically realistic for a given system. For example, a simple extension of the multiscale simulations of granular spheres by Rycroft et al. (2005)
would be to different particle shapes, such as ellipsoids, which have been shown to pack more efficiently than spheres (Donev et al., 2004a). The
only change in the simulation would be to modify
the inter-particle forces in the relaxation step for
a soft-core repulsion with a different shape.
A more challenging and fruitful extension
would be to incorporate mechanics into the multiscale simulation, beyond geometrical packing
constraints. One way to do this may be use
the information about inter-particle forces in the
spot relaxation step to estimate local stresses,
which could then affect the dynamics of spots. It
may also be necessary to move particles directly
in response to mechanical forces, in addition to
the random cooperative displacements caused by
spots. Such extensions seem necessary to describe
forced shear flows in granular and glassy materials. For now, at least we have a reasonable model
for the kinematics of random packings.

Acknowledgements
This work was supported by the U. S. Department of Energy (grant DE-FG02-02ER25530)
and the Norbert Wiener Research Fund and NEC
Fund at MIT. The author is grateful to J. Choi, A.
Kudrolli, R. R. Rosales, C. H. Rycroft for many
stimulating discussions and to A. S. Argon, L.
Bocquet, M. Demkowicz, R. Raghavan for references to the glass literature.
References
Adam, G., Gibbs, J. H., 1965. On the temperature dependence of cooperative relaxation

The Spot Model for random-packings dynamics

15

properties in glass-forming liquids. J. Chem.
Phys. 43, 140–146.

Donev, A., Torquato, S., Stillinger, F. H., Connelly, R., 2004b. Jamming in hard sphere and
disk packings. J. Appl. Phys. 95, 989–999.

Angell, C. A., Ngai, K. L., McKenna, G. B.,
McMillan, P. F., Martin, S. W., 2000. Relaxation in glassforming liquids and amorphous
solids. J. Appl. Phys. 88, 3113–3157.
Argon, A. S., 1979. Plastic deformation in metallic glasses. Acta Metallurgica 27, 47–58.
Baxter, G. W., Behringer, R. P., Fagert, T., Johnson, G. A., 1989. Pattern formation in flowing
sand. Phys. Rev. Lett. 62, 2825.
Bazant, M. Z., Choi, J., Rycroft, C. H., Rosales,
R. R., Kudrolli, A., 2005. A theory of cooperative diffusion in dense granular flow, preprint.
Boutreux, T., Raphaël, E., de Gennes, P.-G.,
1998. Phys. Rev. E 58, 4692.
Bulatov, V. V., Argon, A. S., 1994. A stochastic model for continuum elasto-plastic behavior. Modelling Simul. Mater. Sci. Eng. 2, 167–
222.
Caram, H., Hong, D. C., 1991. Random-walk approach to granular flows. Phys. Rev. Lett. 67,
828–831.
Choi, J., Kudrolli, A., Bazant, M. Z., 2005. Velocity profile of gravity-driven dense granular
flow. J. Phys. A: Condensed MatterTo appear.
Choi, J., Kudrolli, A., Rosales, R. R., Bazant,
M. Z., 2004. Diffusion and mixing in gravity
driven dense granular flows. Phys. Rev. Lett.
92, 174301.
Cohen, M. H., Turnbull, D., 1959. J. Chem. Phys.
31, 1164.
Donati, C., Douglas, J. F., Kob, W., Plimpton,
S. J., Poole, P. H., Glotzer, S. C., 1998. Stringlike cooperative motion in a supercooled liquid.
Phys. Rev. Lett. 80, 2338–2341.
Donev, A., Cisse, I., Sachs, D., Variano, E. A.,
Stillinger, F. H., Connelly, R., Torquato, S.,
Chaikin, P. M., 2004a. Improving the density
of jammed disordered packings using ellipsoids.
Science 303, 990–993.

Ertaş, D., Halsey, T. C., 2002. Granular gravitational collapse and chute flow. Europhys. Lett.
60, 931–937.
Eyring, H., 1936. Viscosity, plasticity, and diffusion as examples of absolute reaction rates. J.
Chem. Phys. 4, 283–291.
Falk, M. L., Langer, J. S., 1998. Dynamics of
viscoplastic deformation in amorphous solids.
Phys. Rev. E 57, 7192–7205.
Götze, W., Sjögren, L., 1992. Rep. Prog. Phys.
55, 241.
Hadjiconstantinou, N. J., Garcia, A. L., Bazant,
M. Z., He, G., 2003. Statistical error in particle simulations of hydrodynamic phenomena.
J. Comp. Phys. 187, 274.
Hansen, J.-P., McDonald, I. R., 1986. Theory of
Simple Liquids. Academic, London.
Jaeger, H. M., Nagel, S. R., Behringer, R. P.,
1996. Granular solids, liquids, and gases. Rev.
Mod. Phys. 68, 1259–1273.
Kansal, A. R., Torquato, S., Stillinger, F. H.,
2002. Diversity of order and densities in
jammed hard-particle packings. Phys. Rev. E
66, 041109.
Kob, W., 1997. The mode-coupling theory of the
glass transition. In: Experimental approaches
to supercooled liquids: Advances and novel applications. ACS Books, Washington, p. 28.
Landry, J. W., Grest, G. S., 2003. Private communication.
Landry, J. W., Grest, G. S., Silbert, L. E., Plimpton, S. J., 2003. Confined granular packings:
structure, stress, and forces. Phys. Rev. E 67,
041303.
Lemaître, A., 2002a. Origin of a repose angle: Kinetics of rearrangements for granular materials.
Phys. Rev. Lett. 89, 064303.

16

M. Z. Bazant

Lemaître, A., 2002b. Rearrangements and dilatency for sheared dense materials. Phys. Rev.
Lett. 89, 195503.

Spaepen, F., 1977. A microscopic mechanism for
steady state inhomogeneous flow in metallic
glasses. Acta Metallurgica 25, 407–415.

Litwiniszyn, J., 1958. Statistical methods in the
mechanics of granular bodies. Rheol. Acta 2/3,
146.

Torquato, S., 2003. Random Heterogeneous Materials. Springer.

Litwiniszyn, J., 1963. The model of a random
walk of particles adapted to researches on problems of mechanics of loose media. Bull. Acad.
Pol. Sci. 11, 593.
Lubachevsky, B. D., Stillinger, F. H., 1990. Geometric properties of random disk packings. J.
Stat. Phys. 60, 561–583.
Mullins, J., 1972. Stochastic theory of particle
flow under gravity. J. Appl. Phys. 43, 665.
Mullins, J., 1974. Experimental evidence for the
stochastic theory of particle flow under gravity.
Powder Technology 9, 29.
Nedderman, R. M., 1991. Statics and Kinematics
of Granular Materials. Nova Science.
O’Hern, C. S., Langer, S. A., Liu, A. J., Nagel,
S. R., 2002. Phys. Rev. Lett. 88, 075507.
O’Hern, C. S., Silbert, L. E., Liu, A. J., Nagel,
S. R., 2003. Jamming at zero temperature and
zero applied stress: The epitome of disorder.
Phys. Rev. E 68, 011306.
Onoda, G. Y., Liniger, E. G., 1990. Random loose
packing of uniform spheres and the dilatancy
onset. Phys. Rev. Lett. 64, 2727.
Orowan, E., 1952. In: Proceedings of the First
International Congress on Applied Mechanics.
AMSE, p. 453.
Risken, H., 1996. The Fokker-Planck Equation.
Springer.
Rycroft, C. H., Bazant, M. Z., Landry, J., Grest,
G. S., 2005. Dynamics of random packings in
granular flow, preprint.
Samadani, A., Pradhan, A., Kudrolli, A., 1999.
Size segregation of granular matter in silo
drainage. Phys. Rev. E 60, 7203–7209.

Torquato, S., Stillinger, F. H., 2001. Multiplicity
of generation, selection, and classification procedures for jammed hard-particle packings. J.
Phys. Chem. 105, 11849.
Torquato, S., Stillinger, F. H., 2003. Local density
fluctuations, hyperuniformity, and order metrics. Phys. Rev. E 68, 041113.
Torquato, S., Truskett, T. M., Debenedetti, P. G.,
2000. Is random close packing of spheres well
defined? Phys. Rev. Lett. 84, 2064.
Tüzün, U., Nedderman, R. M., 1979. Experimental evidence supporting the kinematic modelling of the flow of granular media in the absence of air drag. Powder Technolgy 23, 257.
Weeks, E. R., Crocker, J. C., Levitt, A. C.,
Schofield, A., Weitz, D. A., 2000. Threedimensional direct imaging of structural relaxation near the colloidal glass transition. Science
287, 627–631.
