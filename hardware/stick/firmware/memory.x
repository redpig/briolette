/* nRF52840 memory layout */
MEMORY
{
    /* Softdevice-free layout: full flash available */
    FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}
