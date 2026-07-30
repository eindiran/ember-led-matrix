/* RP2350 memory map and boot block sections, adapted from the rp-hal
 * rp235x-hal-examples memory.x. The RP2350-Matrix carries 16 MB of QSPI
 * flash; 2 MB is declared here as a safe default with ample headroom for
 * this firmware.
 */
MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /* SRAM0-SRAM7 in the striped mapping. */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /* SRAM8/SRAM9 are direct-mapped and left unused here. */
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* Boot ROM info, kept in the first 4K of flash where the Boot ROM
     * (and picotool) can find it.
     */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* Move .text to start after the boot info. */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* Picotool 'Binary Info' entries, located via pointers in the header. */
    .bi_entries : ALIGN(4)
    {
        __bi_entries_start = .;
        KEEP(*(.bi_entries));
        . = ALIGN(4);
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* Boot ROM extra info, after everything else so it can hold a
     * signature.
     */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
        __flash_binary_end = .;
    } > FLASH

} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
