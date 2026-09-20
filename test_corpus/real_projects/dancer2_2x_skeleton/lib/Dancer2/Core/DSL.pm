# Sparse skeleton extracted from Dancer2 (https://github.com/PerlDancer/Dancer2)
# Licensed under the Artistic License 2.0
# Original copyright: Alexis Sukrieh, Sawyer X and contributors
# Trimmed pinned 2.x fixture (#13616): the dsl_keywords map is carried
# verbatim from lib/Dancer2/Core/DSL.pm @ 2.0.1. Proves core-DSL-registry
# behavior ONLY (activation/import and the keyword contract). This fixture
# must never be cited as proof of Dancer2 2.x config, template, serializer,
# or plugin behavior.
package Dancer2::Core::DSL;
use strict;
use warnings;

sub dsl_keywords {

    # the flag meant : 1 = is global, 0 = is not global. global means can be
    # called from anywhere. not global means must be called from within a
    # route handler
    {   any                  => { is_global => 1 },
        app                  => { is_global => 1 },
        captures             => { is_global => 0 },
        config               => { is_global => 1 },
        content              => { is_global => 0 },
        content_type         => { is_global => 0 },
        context              => { is_global => 0 },
        cookie               => { is_global => 0 },
        cookies              => { is_global => 0 },
        dance                => { is_global => 1 },
        dancer_app           => { is_global => 1 },
        dancer_version       => { is_global => 1 },
        dancer_major_version => { is_global => 1 },
        debug                => { is_global => 1 },
        decode_json          => { is_global => 1 },
        del                  => { is_global => 1 },
        delayed              => {
            is_global => 0, prototype => '&@',
        },
        dirname              => { is_global => 1 },
        done                 => { is_global => 0 },
        dsl                  => { is_global => 1 },
        encode_json          => { is_global => 1 },
        engine               => { is_global => 1 },
        error                => { is_global => 1 },
        false                => { is_global => 1 },
        flush                => { is_global => 0 },
        forward              => { is_global => 0 },
        from_dumper          => { is_global => 1 },
        from_json            => { is_global => 1 },
        from_yaml            => { is_global => 1 },
        get                  => { is_global => 1 },
        halt                 => { is_global => 0 },
        header               => { is_global => 0 },
        headers              => { is_global => 0 },
        hook                 => { is_global => 1 },
        info                 => { is_global => 1 },
        log                  => { is_global => 1 },
        mime                 => { is_global => 1 },
        options              => { is_global => 1 },
        param                => { is_global => 0 },
        params               => { is_global => 0 },
        query_parameters     => { is_global => 0 },
        body_parameters      => { is_global => 0 },
        route_parameters     => { is_global => 0 },
        pass                 => { is_global => 0 },
        patch                => { is_global => 1 },
        path                 => { is_global => 1 },
        post                 => { is_global => 1 },
        prefix               => { is_global => 1 },
        prepare_app          => {
            is_global => 1, prototype => '&',
        },
        psgi_app             => { is_global => 1 },
        push_header          => { is_global => 0 },
        push_response_header => { is_global => 0 },
        put                  => { is_global => 1 },
        redirect             => { is_global => 0 },
        request              => { is_global => 0 },
        request_data         => { is_global => 0 },
        request_header       => { is_global => 0 },
        response             => { is_global => 0 },
        response_header      => { is_global => 0 },
        response_headers     => { is_global => 0 },
        runner               => { is_global => 1 },
        send_as              => { is_global => 0 },
        send_error           => { is_global => 0 },
        send_file            => { is_global => 0 },
        session              => { is_global => 0 },
        set                  => { is_global => 1 },
        setting              => { is_global => 1 },
        splat                => { is_global => 0 },
        start                => { is_global => 1 },
        status               => { is_global => 0 },
        template             => { is_global => 1 },
        to_app               => { is_global => 1 },
        to_dumper            => { is_global => 1 },
        to_json              => { is_global => 1 },
        to_yaml              => { is_global => 1 },
        true                 => { is_global => 1 },
        upload               => { is_global => 0 },
        uri_for              => { is_global => 0 },
        uri_for_route        => { is_global => 0 },
        var                  => { is_global => 0 },
        vars                 => { is_global => 0 },
        warning              => { is_global => 1 },
    };
}

1;
