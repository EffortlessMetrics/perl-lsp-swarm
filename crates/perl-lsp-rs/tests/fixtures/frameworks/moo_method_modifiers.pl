package Demo::WithModifiers;
use Moo;

sub save {
    my ($self) = @_;
    return 1;
}

before 'save' => sub {
    my ($self) = @_;
    # validation before save
};

after 'save' => sub {
    my ($self) = @_;
    # cleanup after save
};

around 'save' => sub {
    my ($orig, $self, @args) = @_;
    return $self->$orig(@args);
};

sub process {
    my ($self) = @_;
    $self->save;  # call site — hover on 'save' here to test call-site behavior
}

sub plain_method {
    my ($self) = @_;
    return 'plain';
}
